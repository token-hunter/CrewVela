export function SharedMemoryIcon({ size = 16 }: { size?: number }) {
  return (
    <svg
      aria-hidden="true"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12 4.5C9.4 1.8 5.2 3.6 4.9 7.3C2 8.7 1.4 12.2 3.7 14.7C3 18.2 6 21.5 9.2 20.7C10.4 20.4 11.4 19.8 12 18.9C12.6 19.8 13.6 20.4 14.8 20.7C18 21.5 21 18.2 20.3 14.7C22.6 12.2 22 8.7 19.1 7.3C18.8 3.6 14.6 1.8 12 4.5ZM12 4.5v14.4" />
    </svg>
  );
}
