import type { ReactNode } from "react";

export interface EmptyStateProps {
  className: string;
  icon?: ReactNode;
  title: ReactNode;
  /** Tag used for the title node; variants differ (mcp-empty uses <p>, others <strong>). */
  titleAs?: "p" | "strong" | "span";
  description?: ReactNode;
  descriptionAs?: "small" | "span" | "p";
  action?: ReactNode;
}

export function EmptyState({
  className,
  icon,
  title,
  titleAs = "strong",
  description,
  descriptionAs = "small",
  action,
}: EmptyStateProps) {
  const Title = titleAs;
  const Description = descriptionAs;
  return (
    <div className={className}>
      {icon}
      <Title>{title}</Title>
      {description ? <Description>{description}</Description> : null}
      {action}
    </div>
  );
}
