import type { ReactNode } from "react";
import { Icon, type IconName } from "./icons";
import { radioKeys, radioTab } from "./radios";
import { t } from "./i18n";

/** A setting: what it is on the left, the control on the right. */
export function Option({
  icon,
  label,
  description,
  children,
}: {
  icon: IconName;
  label: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="option">
      <span className="option-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="option-text">
        <strong>{label}</strong>
        {description && <span>{description}</span>}
      </span>
      <span className="option-control">{children}</span>
    </div>
  );
}

/** A few choices side by side. */
export function Choice<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { id: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <span className="choice" role="radiogroup" aria-label={label} onKeyDown={(event) => radioKeys(event)}>
      {options.map((option, index) => (
        <button
          key={option.id}
          type="button"
          role="radio"
          aria-checked={value === option.id}
          tabIndex={radioTab(value === option.id, index, options.some((one) => one.id === value))}
          className={value === option.id ? "chosen" : undefined}
          onClick={() => onChange(option.id)}
        >
          {option.label}
        </button>
      ))}
    </span>
  );
}

/** On or off. */
export function Toggle({ on, onChange, label }: { on: boolean; onChange: (on: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={on} aria-label={label} className="switch" onClick={() => onChange(!on)}>
      <span className={on ? "toggle on" : "toggle"} aria-hidden="true" />
    </button>
  );
}

/** One of several choices, in a dropdown. */
export function Select<T extends string | number>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { id: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <select
      className="settings-select"
      aria-label={label}
      value={String(value)}
      onChange={(event) => {
        const chosen = options.find((option) => String(option.id) === event.target.value);
        if (chosen) onChange(chosen.id);
      }}
    >
      {options.map((option) => (
        <option key={String(option.id)} value={String(option.id)}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

/** A row that opens a page of its own. */
export function LinkRow({ icon, label, detail, onOpen }: { icon: IconName; label: string; detail?: string; onOpen: () => void }) {
  return (
    <button type="button" className="option link-row" onClick={onOpen}>
      <span className="option-icon">
        <Icon name={icon} size={18} />
      </span>
      <span className="option-text">
        <strong>{label}</strong>
        {detail && <span>{detail}</span>}
      </span>
      <Icon name="chevronRight" size={16} />
    </button>
  );
}

/** The title of a page opened from another, with the way back. */
export function SubPageHead({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <div className="subpage-head">
      <button type="button" className="icon-btn" aria-label={t("Back")} title={t("Back")} onClick={onBack}>
        <Icon name="back" size={17} />
      </button>
      <h3>{title}</h3>
    </div>
  );
}
