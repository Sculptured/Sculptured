import { Link } from 'react-router-dom';

const SLIDES = 'https://cs2680.com/slides/5_agent_design_2.pdf';

/** Why to split work across agents (CS2680 Agent Design II, slide 6). */
const REASONS = [
  { name: 'Specialization', why: 'Different instructions, tools or model for a distinct kind of work (research vs. writing vs. review).' },
  { name: 'Isolation', why: 'Different privileges: a reviewer that cannot edit, a builder that cannot publish.' },
  { name: 'Parallelism', why: 'Independent pieces that can run at the same time, so wall-clock time drops.' },
  { name: 'Context', why: 'Each agent keeps a small, focused context instead of one agent carrying everything.' },
];

/** Patterns, each drawn as its boxes in order. `loop` marks a step that can send work back. */
const PATTERNS: { name: string; slide: number; boxes: string[]; loop?: boolean; use: string; watch: string }[] = [
  { name: 'Pipeline with a review loop', slide: 11, boxes: ['Builder', 'Reviewer'], loop: true,
    use: 'Most build-and-check work. Cheapest to debug: one retry edge beats one more agent.', watch: 'Bound the number of revision rounds.' },
  { name: 'Supervisor and workers', slide: 9, boxes: ['Lead', 'Workers ×N', 'Lead integrates'],
    use: 'A task with separable parts that one agent should integrate.', watch: "The lead's context grows fastest; keep worker reports short." },
  { name: 'Independent reviewers', slide: 13, boxes: ['Draft', 'Reviewer A + Reviewer B', 'Editor'],
    use: 'Quality matters more than cost; reviewers must not see each other’s verdicts.', watch: 'Reviewers sharing the same blind spot are not independent.' },
  { name: 'Dynamic dispatch', slide: 16, boxes: ['Coordinator', 'spawns specialists', 'Assembly'],
    use: 'The number of pieces is only known at run time (one worker per chapter or component).', watch: 'Set spawn, depth and token limits before starting.' },
];

/** A sub-task spec (slide 21): what each agent's Job should say. */
const SPEC = [
  ['Objective', 'What this agent must achieve, in one or two sentences.'],
  ['Output', 'The exact file, format or message it hands on.'],
  ['Done when', 'A check anyone can run: tests pass, a file exists with these sections.'],
  ['Boundaries', 'What it must not touch or decide.'],
  ['Budget', 'Its token limit (with or without the agents it spawns).'],
  ['Tools and permissions', 'Restricted for reviewers and anything that should not edit.'],
];

/** Failure modes to design against (slides 24–25, 36). */
const PITFALLS = ['Overlap: two agents do the same work.', 'Gaps: work nobody owns.', 'Hidden dependencies between "parallel" pieces.',
  'Outputs that cannot be merged.', 'Lost global context at the aggregation step.', 'Stragglers holding up everyone else.', 'Agents messaging each other in loops.'];

function Flow({ boxes, loop }: { boxes: string[]; loop?: boolean }) {
  return (
    <p className="design-flow" aria-label={boxes.join(' then ') + (loop ? ', which can send work back' : '')}>
      {boxes.map((b, i) => (
        <span key={b}>
          {i > 0 && <span aria-hidden="true"> → </span>}
          <span className="design-box">{b}</span>
        </span>
      ))}
      {loop && <span aria-hidden="true"> ↺ back to {boxes[0]}</span>}
    </p>
  );
}

/** How to compose specialized agents, from the CS2680 agent design lectures. */
export function AgentDesignGuide() {
  return (
    <article className="release-page agent-design-guide">
      <h1>Designing teams of agents</h1>
      <p>
        Start with <strong>one agent</strong>. A team costs more: multi-agent systems used about 15× the tokens of a chat in
        Anthropic’s research system, and a single agent is often enough. Add agents only for one of four reasons
        (<a href={SLIDES} target="_blank" rel="noreferrer">CS2680 Agent Design II</a>, slide 6):
      </p>
      <ul className="design-reasons">
        {REASONS.map((r) => <li key={r.name}><strong>{r.name}.</strong> {r.why}</li>)}
      </ul>
      <h2>Patterns</h2>
      <ul className="design-patterns">
        {PATTERNS.map((p) => (
          <li key={p.name}>
            <h3>{p.name} <span className="muted">· slide {p.slide}</span></h3>
            <Flow boxes={p.boxes} loop={p.loop} />
            <p><strong>Use for:</strong> {p.use}</p>
            <p><strong>Watch:</strong> {p.watch}</p>
          </li>
        ))}
      </ul>
      <h2>Write each Job like a task spec</h2>
      <dl className="design-spec">
        {SPEC.map(([k, v]) => <div key={k}><dt>{k}</dt><dd>{v}</dd></div>)}
      </dl>
      <h2>What goes wrong</h2>
      <ul>{PITFALLS.map((p) => <li key={p}>{p}</li>)}</ul>
      <h2>In Sculptured</h2>
      <ul>
        <li><strong>Hands work to</strong> arrows build pipelines; a group holds agents that share instructions.</li>
        <li>Give each agent a <strong>token limit</strong> (optionally including the agents it spawns) and set reviewers to <strong>Restricted</strong>.</li>
        <li>An agent saying “done” is a report, not proof: make the last step run the tests.</li>
        <li>Try each pattern from the <Link to="/community/explore?q=cs2680">CS2680 pattern catalog</Link>.</li>
      </ul>
      <p className="muted">Paraphrased from Juncheng Yang’s CS2680 lectures; not an endorsement by the course.</p>
    </article>
  );
}
