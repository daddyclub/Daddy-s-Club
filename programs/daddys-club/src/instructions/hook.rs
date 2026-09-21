//! Облік при передачі бонду (`FR-017`, `FR-038`) — `execute` Transfer Hook.
//!
//! Цю інструкцію не кличе ніхто з наших клієнтів. Її кличе Token-2022 —
//! зсередини кожного `transfer_checked` бонду, після того як переставив
//! баланси, — і саме тому облік діє й тоді, коли переказ іде повз наш
//! застосунок: гук стоїть у мінті, і зняти його нікому (`create_issue`).
//!
//! **Гук фізично спізнюється, а рахує вчасно.** Token-2022 викликає його
//! **після** переміщення токенів, тому баланси тут уже нові, а `FR-017` каже
//! нараховувати «станом на момент безпосередньо перед передачею». Момент
//! відновлюється з `amount`: у відправника було `стало + amount`, в отримувача
//! — `стало − amount`. Обом нараховується `(index − checkpoint) × було / SCALE`
//! в `accrued`, і чекпоінт обох переїжджає на сьогоднішній індекс. Далі
//! `claim` рахує різницю вже від нього на **новому** балансі — і саме так
//! накопичене до передачі лишається за продавцем, а після — за покупцем.
//!
//! **Контракт із `claim`.** Виплата міряє претензію поточним балансом, і це
//! законно рівно тому, що між двома чекпоінтами баланс не змінюється. Цей файл
//! і є тим, що тримає обіцянку: кожна передача бонду або лишає чекпоінти обох
//! сторін, або не відбувається взагалі. Відмова гука валить переказ цілком —
//! Token-2022 не пропускає токени повз CPI, що впало.
//!
//! **Хто має право покликати.** Ніхто. Гук — звичайна інструкція, і
//! атакувальник міг би покликати її напряму, щоб зрушити чекпоінти без
//! переказу. Замок — прапорець `transferring` розширення `TransferHookAccount`:
//! Token-2022 піднімає його на обох рахунках рівно на час переказу й опускає
//! одразу після, а виставити його ззовні нічим — рахунками володіє
//! токен-програма. Перевіряються **обидва** рахунки: одного піднятого прапорця
//! замало, бо тоді рахунок із чужого, справжнього переказу можна було б
//! підсунути в підроблений виклик як другу сторону.
//!
//! **Кому можна передати.** Лише тому, для кого відкрито облік (`FR-038`).
//! Це не перевірка в тілі: список у мінті резолвить адресу чекпоінта
//! отримувача, а `Account<HolderCheckpoint>` вимагає, щоб за нею вже лежав
//! наш акаунт. Немає — Anchor відмовляє на `AccountNotInitialized` ще до
//! обробника, і переказ падає разом із гуком, а не проходить повз облік.
//!
//! **Розкладка набору — чужа.** Порядок акаунтів задає Token-2022: чотири
//! обов'язкові, список, а далі те, що резолвиться зі списку в тому порядку, у
//! якому його записав `hook_account_metas` (`issue.rs`). Структура нижче мусить
//! іти в тому самому порядку — переставити тут поле означає отримати чужий
//! акаунт під своїм іменем. Що порядок той самий, доводить `tests/hook.rs`
//! справжнім `transfer_checked`.

use {
    crate::{
        errors::ClubError,
        instructions::issue::EXTRA_METAS_SEED,
        math,
        state::{HolderCheckpoint, Issue, HOLDER_SEED},
    },
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::spl_token_2022::{
            extension::{
                transfer_hook::TransferHookAccount, BaseStateWithExtensions, StateWithExtensions,
            },
            state::Account as Token2022Account,
        },
        token_interface::{Mint, TokenAccount},
    },
};

/// Чи стоїть рахунок у переказі просто зараз.
///
/// Прапорець живе в розширенні `TransferHookAccount`, яке Token-2022 вимагає
/// від кожного рахунку в мінті з гуком. Рахунок без розширення прапорця не
/// має — і тоді це не переказ: відповідь «ні», а не помилка розбору, бо для
/// замку важливо лише одне — чи піднято прапорець.
fn is_transferring(account: &AccountInfo) -> Result<bool> {
    let data = account.try_borrow_data()?;
    let state = StateWithExtensions::<Token2022Account>::unpack(&data)?;

    Ok(state
        .get_extension::<TransferHookAccount>()
        .map(|flag| bool::from(flag.transferring))
        .unwrap_or(false))
}

#[derive(Accounts)]
pub struct Execute<'info> {
    /// Рахунок відправника. `FR-017`: гук працює лише зсередини переказу, і
    /// доказом переказу є прапорець, який ставить сам Token-2022.
    #[account(
        token::mint = bond_mint,
        constraint = is_transferring(&source_token.to_account_info())? @ ClubError::NotTransferring,
    )]
    pub source_token: InterfaceAccount<'info, TokenAccount>,

    pub bond_mint: InterfaceAccount<'info, Mint>,

    /// Рахунок отримувача — той самий замок, що й у відправника.
    #[account(
        token::mint = bond_mint,
        constraint = is_transferring(&destination_token.to_account_info())? @ ClubError::NotTransferring,
    )]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: authority переказу. Підпис уже перевірив Token-2022, а сам
    /// підписант тут ні на що не впливає: це може бути делегат, а не власник,
    /// тому чекпоінти дерівуються з власників, записаних у самих рахунках.
    pub owner: UncheckedAccount<'info>,

    /// CHECK: список, за яким Token-2022 добрав решту акаунтів. Гук його не
    /// читає — Token-2022 уже прочитав; seeds лише прибивають список до цього
    /// мінта під тим самим seed, під яким його створює `create_issue`.
    #[account(seeds = [EXTRA_METAS_SEED, bond_mint.key().as_ref()], bump)]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    /// Випуск прибитий у списку адресою, а `has_one` замикає кільце з мінтом:
    /// бонд, що переказується, — це бонд саме цього випуску, і індекс береться
    /// з нього.
    #[account(has_one = bond_mint)]
    pub issue: Account<'info, Issue>,

    /// Облік відправника: ті самі seeds, що й у `open_position` та в списку
    /// гука, і власник — той, що записаний у рахунку, а не той, що підписав.
    #[account(
        mut,
        seeds = [HOLDER_SEED, issue.key().as_ref(), source_token.owner.as_ref()],
        bump = holder_source.bump,
    )]
    pub holder_source: Account<'info, HolderCheckpoint>,

    /// Облік отримувача. `FR-038` тримається на самому типі: акаунта немає —
    /// Anchor відмовляє до обробника, і переказ падає разом із гуком.
    #[account(
        mut,
        seeds = [HOLDER_SEED, issue.key().as_ref(), destination_token.owner.as_ref()],
        bump = holder_destination.bump,
    )]
    pub holder_destination: Account<'info, HolderCheckpoint>,
}

/// Переносить в `accrued` те, що належить власникові за балансом `held` від
/// його чекпоінта до `index`, і ставить чекпоінт на `index`.
///
/// Та сама арифметика, що й у виплаті (`math::claimable`), і та сама ціна:
/// ділення вниз, відкинутий залишок лишається у сховищі. Чекпоінт із
/// майбутнього відмовляє тим самим ім'ям, що й `claim`, — це зіпсований облік,
/// а не нуль, і гук не має права тихо його «виправити».
fn settle(holder: &mut HolderCheckpoint, index: u128, held: u128) -> Result<()> {
    require!(
        holder.index_at_checkpoint <= index,
        ClubError::CheckpointAheadOfIndex
    );

    let accrued = math::claimable(
        index,
        holder.index_at_checkpoint,
        held,
        u128::from(holder.accrued),
    )
    .ok_or(ClubError::MathOverflow)?;

    holder.accrued = u64::try_from(accrued).map_err(|_| error!(ClubError::MathOverflow))?;
    holder.index_at_checkpoint = index;

    Ok(())
}

/// Облік при передачі (`FR-017`).
///
/// `amount` — скільки щойно переїхало: Token-2022 передає його в інструкцію, і
/// саме з нього відновлюються баланси «до». Індекс випуску не рухається —
/// його рухає перехоплення, а гук лише фіксує, кому що належало на цю мить.
///
/// **Один гаманець, два рахунки.** Облік ведеться на власника, а не на
/// рахунок, тому переказ між двома рахунками одного гаманця подає той самий
/// чекпоінт двічі. Тоді нараховується один раз — на все, що гаманець тримав
/// на обох рахунках, — а результат кладеться в обидві копії: на виході Anchor
/// записує кожну, і яка з них ляже останньою, значення не має.
pub fn execute(ctx: Context<Execute>, amount: u64) -> Result<()> {
    let index = ctx.accounts.issue.payout_index;
    let amount = u128::from(amount);

    // Баланси вже нові — «до» відновлюється з `amount` (docs/PLAN.md, потік 3).
    let sender_held = u128::from(ctx.accounts.source_token.amount)
        .checked_add(amount)
        .ok_or(ClubError::MathOverflow)?;
    let recipient_held = u128::from(ctx.accounts.destination_token.amount)
        .checked_sub(amount)
        .ok_or(ClubError::MathOverflow)?;

    if ctx.accounts.holder_source.key() == ctx.accounts.holder_destination.key() {
        let held = sender_held
            .checked_add(recipient_held)
            .ok_or(ClubError::MathOverflow)?;
        settle(&mut ctx.accounts.holder_source, index, held)?;

        let settled: HolderCheckpoint = (*ctx.accounts.holder_source).clone();
        *ctx.accounts.holder_destination = settled;

        return Ok(());
    }

    settle(&mut ctx.accounts.holder_source, index, sender_held)?;
    settle(&mut ctx.accounts.holder_destination, index, recipient_held)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*, anchor_lang::Discriminator, spl_discriminator::SplDiscriminate,
        spl_transfer_hook_interface::instruction::ExecuteInstruction,
    };

    /// Дискримінатор задає Token-2022, а не ми: він шле `execute` під
    /// восьмибайтовим тегом інтерфейсу гука, і якщо наш тег інший, жодна
    /// передача бонду не дійде до обробника. Замінити тег у `lib.rs` на
    /// звичайний Anchor-овий — sha256 від імені — найлегша поломка, і на
    /// збірці вона не видна: програма збереться, а Token-2022 упиратиметься
    /// у fallback.
    #[test]
    fn execute_answers_to_the_discriminator_token_2022_sends() {
        assert_eq!(
            crate::instruction::Execute::DISCRIMINATOR,
            ExecuteInstruction::SPL_DISCRIMINATOR_SLICE
        );
    }

    #[test]
    fn settling_moves_the_earned_share_into_accrued_and_the_checkpoint_forward() {
        let mut holder = HolderCheckpoint {
            issue: Pubkey::default(),
            owner: Pubkey::default(),
            index_at_checkpoint: 0,
            accrued: 5,
            claimed_total: 0,
            bump: 0,
        };

        // 48e9 × 1e10 / 1e12 = 480e6, плюс уже нараховані 5.
        settle(&mut holder, 48_000_000_000, 10_000_000_000).unwrap();

        assert_eq!(holder.accrued, 480_000_005);
        assert_eq!(holder.index_at_checkpoint, 48_000_000_000);
    }

    #[test]
    fn settling_on_a_zero_balance_only_moves_the_checkpoint() {
        let mut holder = HolderCheckpoint {
            issue: Pubkey::default(),
            owner: Pubkey::default(),
            index_at_checkpoint: 0,
            accrued: 0,
            claimed_total: 0,
            bump: 0,
        };

        settle(&mut holder, 48_000_000_000, 0).unwrap();

        assert_eq!(holder.accrued, 0);
        assert_eq!(holder.index_at_checkpoint, 48_000_000_000);
    }

    #[test]
    fn a_checkpoint_ahead_of_the_index_is_refused_by_name() {
        let mut holder = HolderCheckpoint {
            issue: Pubkey::default(),
            owner: Pubkey::default(),
            index_at_checkpoint: 2,
            accrued: 0,
            claimed_total: 0,
            bump: 0,
        };

        let error = settle(&mut holder, 1, 10).unwrap_err();

        assert_eq!(
            ProgramError::from(error),
            ProgramError::Custom(u32::from(ClubError::CheckpointAheadOfIndex))
        );
        assert_eq!(holder.index_at_checkpoint, 2, "відмова не має чіпати облік");
    }
}
