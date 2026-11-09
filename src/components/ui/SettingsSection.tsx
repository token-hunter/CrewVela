import type { ReactNode } from "react";

export interface SettingsSectionProps {
  className?: string;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}

export function SettingsSection({ className, title, description, children }: SettingsSectionProps) {
  return (
    <section className={className}>
      <h3>{title}</h3>
      {description ? <p>{description}</p> : null}
      {children}
    </section>
  );
}
