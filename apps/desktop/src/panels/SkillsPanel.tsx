import type { SkillCatalogueView, SkillSource, SkillSummaryView } from '../bridge-types';

// The project's skill catalogue.
//
// The TUI's `/skills` command inspects the effective skills and their
// shadowed alternatives (`src/tui/command.rs:663-664`); the daemon
// answers a frontend with the catalogue itself — the skill's name, its
// one-line description, and the namespace it was discovered in
// (`crates/codegg-protocol/src/skill.rs:33-59`). The catalogue is
// metadata only: no skill body and no filesystem path crosses the
// bridge, so the panel can draw the list without knowing where the
// skills live.
//
// The list is grouped by the source the daemon reported, because the
// source is what tells a reader which of two same-named skills wins.
//
// A `null` catalogue means the host has not read one yet — not that the
// project has no skills. The panel says so instead of drawing an empty
// list, because "no skills" and "not known yet" are different claims.

export interface SkillsPanelProps {
  /** The project's catalogue, or null when not read yet. */
  catalogue: SkillCatalogueView | null;
  /** Whether a read is in flight. */
  busy?: boolean;
  /** The last read's failure, if it failed. */
  error?: string | null;
  /** Re-read the catalogue from the daemon. */
  onRefresh?: () => void;
}

interface SkillGroup {
  source: SkillSource;
  skills: SkillSummaryView[];
}

/// Group the catalogue by the source the daemon reported, keeping the
/// daemon's own order of first appearance for both the groups and the
/// skills inside them.
export function groupSkillsBySource(skills: SkillSummaryView[]): SkillGroup[] {
  const groups: SkillGroup[] = [];
  for (const skill of skills) {
    const existing = groups.find((group) => group.source === skill.source);
    if (existing) {
      existing.skills.push(skill);
    } else {
      groups.push({ source: skill.source, skills: [skill] });
    }
  }
  return groups;
}

export function SkillsPanel({
  catalogue,
  busy = false,
  error = null,
  onRefresh,
}: SkillsPanelProps) {
  const groups = catalogue ? groupSkillsBySource(catalogue.skills) : [];
  return (
    <section className="surface" data-testid="skills-surface">
      <h3>
        Skills
        {catalogue && <small data-testid="skill-count">{catalogue.skills.length}</small>}
      </h3>
      {onRefresh && (
        <button data-testid="skills-refresh-button" disabled={busy} onClick={onRefresh}>
          {busy ? 'Refreshing…' : 'Refresh skills'}
        </button>
      )}
      {error && (
        // A failed read is reported here rather than as a route error: it
        // is a skills-surface condition, and the catalogue the host last
        // gave us is still the truth.
        <p role="status" data-testid="skills-error">
          Skill catalogue read failed: {error}
        </p>
      )}
      {catalogue === null ? (
        <p className="muted" data-testid="skills-unknown">
          Skill catalogue not read yet.
        </p>
      ) : catalogue.skills.length === 0 ? (
        <p className="muted" data-testid="skills-empty">
          No skills discovered.
        </p>
      ) : (
        groups.map((group) => (
          <section key={group.source} data-testid="skill-group" data-source={group.source}>
            <h4 data-testid="skill-group-title">{group.source}</h4>
            <ul data-testid="skill-list">
              {group.skills.map((skill) => (
                <li
                  key={skill.name}
                  data-testid="skill-item"
                  data-skill-name={skill.name}
                  data-skill-source={skill.source}
                >
                  <strong data-testid="skill-name">{skill.name}</strong>
                  <span data-testid="skill-description">{skill.description}</span>
                  <span data-testid="skill-resources">
                    {skill.resourceCount} resource(s)
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
      {catalogue?.truncated && (
        // The daemon capped the catalogue: the panel says so rather than
        // implying the list is complete.
        <p className="muted" data-testid="skills-truncated">
          Showing {catalogue.skills.length} of {catalogue.total} skill(s).
        </p>
      )}
    </section>
  );
}
