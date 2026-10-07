import type { ReactNode } from "react";
import { EmptyIcon } from "../shell/icons";

/**
 * What every empty list shows (ADR 0010 decision 6): an icon, one plain sentence and at most one
 * button. No jargon: "You have no cards yet", not "Collection has 0 notes".
 */
export function EmptyState({
  icon = <EmptyIcon />,
  children,
  action,
}: {
  icon?: ReactNode;
  children: ReactNode;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className="empty-state">
      {icon}
      <p>{children}</p>
      {action && (
        <button type="button" className="button button-primary" onClick={action.onClick}>
          {action.label}
        </button>
      )}
    </div>
  );
}
