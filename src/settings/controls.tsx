import { useId, type ReactNode } from "react";

export function Section({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="section">
      {title && <h2 className="section-title">{title}</h2>}
      <div className="card">{children}</div>
    </section>
  );
}

/** One setting: text on the left, its control on the right. */
export function Row({
  title,
  description,
  htmlFor,
  labelId,
  children,
}: {
  title: string;
  description?: ReactNode;
  htmlFor?: string;
  labelId?: string;
  children: ReactNode;
}) {
  return (
    <div className="row">
      <div className="row-text">
        <label className="row-title" htmlFor={htmlFor} id={labelId}>
          {title}
        </label>
        {description && <div className="row-description">{description}</div>}
      </div>
      <div className="row-control">{children}</div>
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  labelledBy,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  labelledBy?: string;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" />
    </button>
  );
}

/** A switch row: the label is the switch's accessible name. */
export function SwitchRow(props: { title: string; description?: ReactNode; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  const id = useId();
  return (
    <Row title={props.title} description={props.description} labelId={id}>
      <Switch checked={props.checked} onChange={props.onChange} labelledBy={id} disabled={props.disabled} />
    </Row>
  );
}

export function SliderRow({
  title,
  description,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  title: string;
  description?: ReactNode;
  value: number;
  min: number;
  max: number;
  step: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
}) {
  const id = useId();
  return (
    <Row title={title} description={description} htmlFor={id}>
      <div className="slider">
        <input
          id={id}
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          aria-valuetext={format(value)}
          onChange={(e) => onChange(Number(e.target.value))}
          style={{ ["--fill" as string]: `${((value - min) / (max - min)) * 100}%` }}
        />
        <output htmlFor={id} className="slider-value">
          {format(value)}
        </output>
      </div>
    </Row>
  );
}

export function SelectRow<T extends string>({
  title,
  description,
  value,
  options,
  onChange,
}: {
  title: string;
  description?: ReactNode;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  const id = useId();
  return (
    <Row title={title} description={description} htmlFor={id}>
      <select id={id} className="select" value={value} onChange={(e) => onChange(e.target.value as T)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </Row>
  );
}

/** A list of named items, each with an on/off switch; `empty` shows when there are none. */
export function ToggleList({
  items,
  empty,
  onToggle,
}: {
  items: { key: string; label: string; on: boolean; note?: string }[];
  empty: ReactNode;
  onToggle: (key: string, on: boolean) => void;
}) {
  if (items.length === 0) return <div className="empty">{empty}</div>;
  return (
    <ul className="toggle-list">
      {items.map((item) => (
        <li key={item.key} className="toggle-item">
          <span className="avatar" aria-hidden style={{ background: avatarColor(item.label) }}>
            {item.label.charAt(0).toUpperCase()}
          </span>
          <span className="toggle-label">
            {item.label}
            {item.note && <span className="toggle-note">{item.note}</span>}
          </span>
          <Switch checked={item.on} onChange={(on) => onToggle(item.key, on)} label={`Show ${item.label}`} />
        </li>
      ))}
    </ul>
  );
}

/** Same hue logic as the pill's notification fallback icon, so apps look alike in both. */
function avatarColor(name: string) {
  const hue = [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
  return `hsl(${hue} 65% 52%)`;
}
