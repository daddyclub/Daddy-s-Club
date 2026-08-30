import type { ButtonHTMLAttributes, ReactNode } from 'react';

interface BoardButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  variant?: 'solid' | 'line';
}

const BoardButton = ({
  children,
  variant = 'solid',
  className = '',
  ...rest
}: BoardButtonProps) => {
  const base =
    'inline-flex items-center justify-center border px-3 py-[7px] text-[10px] uppercase tracking-[0.2em] transition-colors disabled:cursor-not-allowed disabled:opacity-45';
  const look =
    variant === 'solid'
      ? 'border-board-accent bg-board-accent text-board-bg hover:bg-board-ink hover:border-board-ink'
      : 'border-board-ink text-board-ink hover:bg-board-ink hover:text-board-bg';
  return (
    <button type="button" className={`${base} ${look} ${className}`} {...rest}>
      {children}
    </button>
  );
};

export default BoardButton;
