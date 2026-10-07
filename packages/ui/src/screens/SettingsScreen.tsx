import { Link, PageHeading } from "../router";
import { DeveloperIcon } from "../shell/icons";
import { type ThemePreference, useTheme } from "../theme";

const choices: { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsScreen({ developerTools }: { developerTools: boolean }) {
  const { preference, setPreference } = useTheme();
  return (
    <>
      <PageHeading>Settings</PageHeading>
      <fieldset className="group">
        <legend>Appearance</legend>
        {choices.map((choice) => (
          <label key={choice.value} className="choice">
            <input
              type="radio"
              name="theme"
              value={choice.value}
              checked={preference === choice.value}
              onChange={() => setPreference(choice.value)}
            />
            {choice.label}
          </label>
        ))}
        <p className="hint">System follows your device. This choice is kept on this device only.</p>
      </fieldset>
      {developerTools && (
        <Link path="/settings/developer" className="row-link">
          <DeveloperIcon />
          Developer tools
        </Link>
      )}
    </>
  );
}
