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
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      xmlns="http://www.w3.org/2000/svg"
    >
      <circle cx="12" cy="12" r="9.25" />
      <path d="M12 6.5V12l3.75 2.75" />
    </svg>
  );
}
