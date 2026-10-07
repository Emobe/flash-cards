import { useId, useState } from "react";

const MAX_SUGGESTIONS = 6;

/** Tags have no spaces, so a space ends one. */
export function splitTags(text: string): string[] {
  return text.split(/[\s,]+/).filter((tag) => tag.length > 0);
}

/** `tags` plus what is typed in the box and not yet turned into a chip. */
export function withTyped(tags: string[], typed: string): string[] {
  const result = [...tags];
  for (const tag of splitTags(typed)) {
    if (!result.some((have) => have.toLowerCase() === tag.toLowerCase())) result.push(tag);
  }
  return result;
}

/**
 * Tags as chips with suggestions from the tags already in use (prefix match, ignoring case). The
 * text typed so far belongs to the screen, so Add can include it.
 */
export function TagInput({
  tags,
  typed,
  known,
  onTags,
  onTyped,
}: {
  tags: string[];
  typed: string;
  /** Every tag in the collection, for suggestions. */
  known: string[];
  onTags: (tags: string[]) => void;
  onTyped: (text: string) => void;
}) {
  const id = useId();
  const [focused, setFocused] = useState(false);

  const prefix = typed.trim().toLowerCase();
  const suggestions = prefix
    ? known
        .filter(
          (tag) =>
            tag.toLowerCase().startsWith(prefix) &&
            !tags.some((have) => have.toLowerCase() === tag.toLowerCase()),
        )
        .slice(0, MAX_SUGGESTIONS)
    : [];

  function commit(text: string) {
    onTags(withTyped(tags, text));
    onTyped("");
  }

  return (
    <div className="tag-input">
      <label htmlFor={id} className="add-label">
        Tags
      </label>
      <div className="tag-box">
        {tags.length > 0 && (
          <ul className="tag-chips" aria-label="Tags on this note">
            {tags.map((tag) => (
              <li key={tag}>
                <span>{tag}</span>
                <button
                  type="button"
                  aria-label={`Remove tag ${tag}`}
                  onClick={() => onTags(tags.filter((have) => have !== tag))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
        <input
          id={id}
          type="text"
          value={typed}
          placeholder={tags.length === 0 ? "Add a tag" : "Add another"}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          enterKeyHint="done"
          onChange={(event) => {
            const text = event.target.value;
            // A space or a comma ends the tag.
            if (/[\s,]/.test(text) && text.trim().length > 0) commit(text);
            else onTyped(text.replace(/[\s,]/g, ""));
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && typed.trim()) {
              event.preventDefault();
              commit(typed);
            } else if (event.key === "Backspace" && typed === "" && tags.length > 0) {
              onTags(tags.slice(0, -1));
            }
          }}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false);
            if (typed.trim()) commit(typed);
          }}
        />
      </div>
      {focused && suggestions.length > 0 && (
        <ul className="tag-suggestions" aria-label="Tag suggestions">
          {suggestions.map((tag) => (
            <li key={tag}>
              <button
                type="button"
                // Pressing it must not blur the input first, or the typed text would become a tag.
                onMouseDown={(event) => event.preventDefault()}
                onPointerDown={(event) => event.preventDefault()}
                onClick={() => commit(tag)}
              >
                {tag}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
