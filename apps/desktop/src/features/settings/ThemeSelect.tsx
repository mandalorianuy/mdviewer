export type ThemePreference = "system" | "light" | "dark";

interface ThemeSelectProps {
  value: ThemePreference;
  onChange(value: ThemePreference): void;
}

export function ThemeSelect({ value, onChange }: ThemeSelectProps) {
  return (
    <label className="theme-select">
      <span>Tema</span>
      <select aria-label="Tema" value={value} onChange={(event) => onChange(event.target.value as ThemePreference)}>
        <option value="system">Sistema</option>
        <option value="light">Claro</option>
        <option value="dark">Oscuro</option>
      </select>
    </label>
  );
}
