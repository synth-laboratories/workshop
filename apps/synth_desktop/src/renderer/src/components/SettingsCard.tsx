import type { ReactNode } from "react";
import { useId } from "react";

type CardProps = {
	/** Omit for cards whose content brings its own heading. */
	title?: string;
	description?: string;
	/** Right-aligned header content (badges, buttons). */
	actions?: ReactNode;
	/** Accessible name for headerless cards named by the page title. */
	ariaLabel?: string;
	children: ReactNode;
	testId?: string;
	className?: string;
};

/**
 * Shared Settings surface: bordered card with a tinted header strip and a
 * body whose `SettingsRow` children divide into consistent rows.
 */
export function SettingsCard({ title, description, actions, ariaLabel, children, testId, className }: CardProps) {
	const headingId = useId();
	return (
		<section
			className={`settings-card${className ? ` ${className}` : ""}`}
			data-testid={testId}
			aria-labelledby={title ? headingId : undefined}
			aria-label={title ? undefined : ariaLabel}
		>
			{title ? (
				<header className="settings-card-head">
					<div>
						<h3 id={headingId}>{title}</h3>
						{description ? <p>{description}</p> : null}
					</div>
					{actions}
				</header>
			) : null}
			<div className="settings-card-body">{children}</div>
		</section>
	);
}

type RowProps = {
	label: string;
	description?: string;
	/** Associates the visible label with a form control by id. */
	htmlFor?: string;
	children: ReactNode;
};

export function SettingsRow({ label, description, htmlFor, children }: RowProps) {
	return (
		<div className="settings-item">
			<div className="settings-item-copy">
				{htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span>{label}</span>}
				{description ? <p>{description}</p> : null}
			</div>
			<div className="settings-item-control">{children}</div>
		</div>
	);
}
