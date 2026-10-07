import { CoreError } from "core-client";
import { type ReactNode, useEffect, useState } from "react";
import { useCore } from "../core";
import { DivideForm } from "../DivideForm";
import { Link, PageHeading } from "../router";
import { SchedulingSpike } from "../SchedulingSpike";
import { BottomAction } from "../shell/slot";
import "../styles/spikes.css";

/**
 * The spikes from Phase 0 and 1, kept as test rigs until their replacements land (the card
 * sandbox spike went in 2.3, backups follow in 2.6). Only debug builds link here (ADR 0010 decision 7).
 */
export function DeveloperScreen({ extraTools }: { extraTools?: ReactNode }) {
  return (
    <>
      <PageHeading>Developer tools</PageHeading>
      <Link path="/settings">Back to Settings</Link>
      <Versions />
      <KeyboardTest />
      <SampleCards />
      <DivideForm />
      <SchedulingSpike />
      {extraTools}
    </>
  );
}

/** Nothing can add cards until step 2.4, so this makes some to study (debug builds only). */
function SampleCards() {
  const core = useCore();
  const [message, setMessage] = useState<string | null>(null);
  return (
    <section className="dev-section" aria-label="Sample cards">
      <h2>Sample cards</h2>
      <p>
        Adds the decks Sample and Sample::Sound: words, a reversed pair, a cloze, sound and a
        picture.
      </p>
      <button
        type="button"
        onClick={() =>
          core.call("debugAddSampleCards", null).then(
            ({ notes }) => setMessage(`Added ${notes} notes. Open Decks to study them.`),
            (error: unknown) =>
              setMessage(error instanceof CoreError ? error.message : "Something went wrong."),
          )
        }
      >
        Add sample cards
      </button>
      {message && <p role="status">{message}</p>}
    </section>
  );
}

function Versions() {
  const core = useCore();
  const [lines, setLines] = useState(["Loading versions..."]);

  useEffect(() => {
    let current = true;
    const problem = (error: unknown) =>
      error instanceof CoreError ? error.message : "Something went wrong. Try again.";
    Promise.all([
      core.call("getCoreInfo", null).then(
        (info) => `Core version ${info.coreVersion}`,
        (error: unknown) => problem(error),
      ),
      core.call("getCollectionInfo", null).then(
        (info) =>
          `Collection storage version ${info.schemaVersion} (this build supports up to ${info.supportedSchemaVersion}).`,
        (error: unknown) => problem(error),
      ),
    ]).then((result) => current && setLines(result));
    return () => {
      current = false;
    };
  }, [core]);

  return (
    <section className="dev-section" aria-label="Versions">
      {lines.map((line) => (
        <p key={line}>{line}</p>
      ))}
    </section>
  );
}

/** Checks the shell's keyboard handling on a phone: focus the field and look at the button. */
function KeyboardTest() {
  const [text, setText] = useState("");
  return (
    <section className="dev-section">
      <label htmlFor="keyboard-test">Keyboard test</label>
      <input id="keyboard-test" value={text} onChange={(e) => setText(e.target.value)} />
      <BottomAction>
        <button type="button" className="button button-primary" onClick={() => setText("")}>
          Clear the field
        </button>
      </BottomAction>
    </section>
  );
}
