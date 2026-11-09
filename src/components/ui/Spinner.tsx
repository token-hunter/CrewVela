export function Spinner({ className }: { className?: string }) {
  return <span className={className ? `spin ${className}` : "spin"} />;
}
