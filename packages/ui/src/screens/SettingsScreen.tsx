import { useEffect, useState } from "react";
import { useCore } from "../core";
import { Link, PageHeading } from "../router";
import { DeveloperIcon } from "../shell/icons";
import { type ThemePreference, useTheme } from "../theme";
import { BackupsSection } from "./BackupsSection";

const choices: { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsScreen({
  developerTools,
  build,
}: {
  developerTools: boolean;
  /** Which source build this is (a short commit, `-dirty`, or `dev`), shown next to the version. */
  build?: string;
}) {
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
      <BackupsSection />
      <AboutGroup build={build} />
      {developerTools && (
        <Link path="/settings/developer" className="row-link">
          <DeveloperIcon />
          Developer tools
        </Link>
      )}
    </>
  );
}

/**
 * "Version 0.1.0 (a1b2c3d)": the app version is the core's (one workspace version, ADR 0012). The
 * group stays out of the way until the version is known, and when the call fails.
 */
function AboutGroup({ build }: { build?: string }) {
  const core = useCore();
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    core.call("getCoreInfo", null).then(
      (info) => current && setVersion(info.coreVersion),
      () => undefined,
    );
    return () => {
      current = false;
    };
  }, [core]);
  if (version === null) return null;
  return (
    <fieldset className="group">
      <legend>About</legend>
      <p className="about-version">
        {build ? `Version ${version} (${build})` : `Version ${version}`}
      </p>
    </fieldset>
  );
}
