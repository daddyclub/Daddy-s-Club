//! Позиція інвестора: відкриття обліку (`FR-038`).
//!
//! `HolderCheckpoint` — це не запис «про всяк випадок», а перепустка. Список
//! акаунтів гука, покладений у мінт при створенні випуску, резолвить чекпоінти
//! обох сторін переказу; акаунта немає — резолв не сходиться, і Token-2022
//! відхиляє передачу цілком, замість того щоб пропустити її повз облік. Тому
//! `open_position` мусить створювати рівно той PDA, який гук шукає, і жодного
//! іншого: адреса зафіксована в `hook_account_metas` і переписати її нічим.
//!
//! Відкриття дозвільне — платник і власник розділені, і власник не підписує.
//! Це не послаблення: усі поля пише програма, тому відкритий кимось стороннім
//! облік нічим не відрізняється від відкритого самим власником, а от вторинці
//! (`FR-024`) розділення дає відкрити облік покупця в тій самій транзакції, що
//! й купівля.
//!
//! Чого тут навмисно немає — токен-акаунта під бонд. Облік і рахунок це різні
//! речі: рахунок заводить Token-2022 звичайною ATA, а `FR-038` говорить саме
//! про облік.

use {
    crate::state::{HolderCheckpoint, Issue, HOLDER_SEED},
    anchor_lang::prelude::*,
};

#[derive(Accounts)]
pub struct OpenPosition<'info> {
    /// Seeds випуску містять `seq` і джерело, яких у цьому наборі немає, — але
    /// перевіряти їх тут і не треба: `Account<Issue>` уже вимагає власника-нашу
    /// програму й дискримінатор `Issue`, а такий акаунт з'являється лише з
    /// `create_issue`. Підробити його поза програмою нічим.
    pub issue: Account<'info, Issue>,

    /// Ті самі seeds, що їх резолвить список гука: випуск і власник. Збіг
    /// доводить `the_hook_resolves_to_the_ledger_this_instruction_creates`.
    #[account(
        init,
        payer = payer,
        space = 8 + HolderCheckpoint::INIT_SPACE,
        seeds = [HOLDER_SEED, issue.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub holder: Account<'info, HolderCheckpoint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: власник обліку. Підпису не вимагає — відкриття дозвільне
    /// (`FR-038`), і чужий облік однаково веде програма, а не той, хто його
    /// оплатив. Обмежувати власника системним акаунтом теж не можна: бонд
    /// цілком може лежати на PDA чужої програми.
    pub owner: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn open_position(ctx: Context<OpenPosition>) -> Result<()> {
    let issue = ctx.accounts.issue.key();
    let owner = ctx.accounts.owner.key();
    // Свіжий облік починається з сьогоднішнього індексу, а не з нуля: різницю
    // рахує `FR-016`, і чекпоінт у нулі віддав би новому власникові всі
    // виплати, що накопичились до того, як він тут з'явився.
    let index_at_checkpoint = ctx.accounts.issue.payout_index;
    let bump = ctx.bumps.holder;

    let holder = &mut ctx.accounts.holder;
    holder.issue = issue;
    holder.owner = owner;
    holder.index_at_checkpoint = index_at_checkpoint;
    holder.accrued = 0;
    holder.claimed_total = 0;
    holder.bump = bump;

    Ok(())
}
