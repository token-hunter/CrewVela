interface Props {
  size?: number;
  className?: string;
}

export function AutomationIcon({ size = 17, className }: Props) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path d="M12 8V4H8" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="4" y="8" width="16" height="12" rx="2.5" stroke="currentColor" strokeWidth="1.7" />
      <path d="M2 14H4M20 14H22" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
      <path d="M9 13V15M15 13V15" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    </svg>
  );
}
