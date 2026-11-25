import clsx from 'clsx';
import type { ButtonHTMLAttributes, ReactNode } from 'react';

interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'size'> {
  children: ReactNode;
  size?: string;
  variant?: string;
}

const Button = ({
  children,
  onClick,
  disabled,
  className = '',
  size: _size,
  variant: _variant,
  ...props
}: ButtonProps) => {
  const baseClasses = `
    flex items-center justify-center gap-2
    font-semibold py-2 px-4 rounded-md
    text-md
    disabled:opacity-50 disabled:cursor-not-allowed
  `;

  const hasSurfaceBg = className.includes('bg-surface');

  // bg-surface buttons get theme-primary text (readable on the light surface);
  // default buttons keep the accent background with button-text (white) —
  // previously the white text stayed on surface backgrounds too, making
  // surface buttons invisible in light themes (settings consent buttons).
  const combinedClasses = clsx(
    baseClasses,
    {
      'bg-accent text-button-text': !hasSurfaceBg,
      'bg-surface text-text-primary': hasSurfaceBg,
    },
    className,
  );

  return (
    <button onClick={onClick} disabled={disabled} className={combinedClasses} {...props}>
      {children}
    </button>
  );
};

export default Button;
