import type { ReactNode } from 'react';
import { useId } from 'react';
import type { GenerateOptions } from '../api';
import { Btn } from '../shell/ui';
import { TextArea } from '../shell/TextArea';

export const CLI_NAME: Record<string, string> = { claude: 'Claude Code', codex: 'Codex' };

type Computers = { id: string; name: string }[];
type Set = <K extends keyof GenerateOptions>(k: K, v: GenerateOptions[K]) => void;
type OptionsProps = { opts: GenerateOptions; set: Set; computers: Computers; signedIn: string[] | null; count: 1 | 3; setCount: (c: 1 | 3) => void; askFirst: boolean; setAskFirst: (b: boolean) => void };

function ComputerSelect({ opts, set, computers }: { opts: GenerateOptions; set: Set; computers: Computers }) {
  if (computers.length < 2) return null;
  return (
    <label>
      Computer{' '}
      <select value={opts.host ?? ''} onChange={(e) => set('host', e.target.value)}>
        {computers.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function CliSelect({ opts, set, signedIn }: { opts: GenerateOptions; set: Set; signedIn: string[] | null }) {
  return (
    <label>
      CLI{' '}
      <select value={opts.cli} onChange={(e) => set('cli', e.target.value as GenerateOptions['cli'])}>
        {(['claude', 'codex'] as const).map((c) => (
          <option key={c} value={c}>
            {CLI_NAME[c]}
            {signedIn && !signedIn.includes(c) ? ' (not signed in)' : ''}
          </option>
        ))}
      </select>
    </label>
  );
}

function ShapeSelects({ opts, set }: { opts: GenerateOptions; set: Set }) {
  return <>
    <label>
      Team size{' '}
      <select value={opts.size ?? ''} onChange={(e) => set('size', (e.target.value || null) as GenerateOptions['size'])}>
        <option value="2-3">2–3 agents</option>
        <option value="4-6">4–6 agents</option>
        <option value="7-10">7–10 agents</option>
      </select>
    </label>
    <label>
      Agent kinds{' '}
      <select value={opts.kinds ?? ''} onChange={(e) => set('kinds', (e.target.value || null) as GenerateOptions['kinds'])}>
        <option value="">Same as the CLI</option>
        <option value="claude">Claude</option>
        <option value="codex">Codex</option>
        <option value="mixed">Mixed</option>
      </select>
    </label>
    <label>
      Shape{' '}
      <select value={opts.shape ?? ''} onChange={(e) => set('shape', (e.target.value || null) as GenerateOptions['shape'])}>
        <option value="">Let the AI choose</option>
        <option value="chain">A chain</option>
        <option value="lead">A lead with helpers</option>
        <option value="reviewers">Builders with reviewers</option>
      </select>
    </label>
  </>;
}

function OptionsGrid({ opts, set, computers, signedIn, count, setCount, askFirst, setAskFirst }: OptionsProps) {
  return (
    <details className="gen-options">
      <summary>Options</summary>
      <div className="gen-grid">
        <ComputerSelect opts={opts} set={set} computers={computers} />
        <CliSelect opts={opts} set={set} signedIn={signedIn} />
        <label>
          Model <input className="fld-input" value={opts.model ?? ''} placeholder="the CLI's default" onChange={(e) => set('model', e.target.value.trim() || null)} />
        </label>
        <ShapeSelects opts={opts} set={set} />
        <label>
          Alternatives{' '}
          <select value={count} onChange={(e) => setCount(e.target.value === '1' ? 1 : 3)}>
            <option value="3">3</option>
            <option value="1">1</option>
          </select>
        </label>
        <label className="gen-check">
          <input type="checkbox" checked={opts.checks} onChange={(e) => set('checks', e.target.checked)} /> Add check commands
        </label>
        <label className="gen-check">
          <input type="checkbox" checked={askFirst} onChange={(e) => setAskFirst(e.target.checked)} /> Ask me questions first
        </label>
      </div>
    </details>
  );
}

type FormProps = OptionsProps & { prompt: string; setPrompt: (p: string) => void; busy: boolean; errorBox: ReactNode; onSubmit: () => void; onCancel: () => void };

/** The first step: the description, the options, and what it will use. */
export function DescribeForm({ prompt, setPrompt, busy, errorBox, onSubmit, onCancel, ...options }: FormProps) {
  const ids = useId();
  const { opts, askFirst } = options;
  const cliName = CLI_NAME[opts.cli];
  return (
    <form className="gen-describe" onSubmit={(e) => { e.preventDefault(); onSubmit(); }}>
      <label htmlFor={`${ids}-prompt`} className="gen-label">
        Describe the team
      </label>
      <TextArea id={`${ids}-prompt`} className="fld-input" rows={4} value={prompt} placeholder="e.g. Rebuild our docs site in Astro: one agent plans the pages, two write them, one reviews" onChange={(e) => setPrompt(e.target.value)} />
      <OptionsGrid {...options} />
      <p className="muted gen-cost">
        Uses your {cliName} plan · {opts.model ?? 'default model'}.{' '}
        {opts.cli === 'claude'
          ? 'Claude Code runs with no tools or MCP servers: it sees only this description.'
          : 'Codex runs read-only: it can read files on this computer but not change them.'}
      </p>
      {errorBox}
      <div className="gen-actions">
        <Btn onClick={onCancel}>Cancel</Btn>
        <Btn type="submit" variant="primary" disabled={!prompt.trim() || busy}>
          {askFirst ? 'Next: questions' : 'Generate'}
        </Btn>
      </div>
      {busy && <p role="status">Asking {cliName}… this usually takes under a minute.</p>}
    </form>
  );
}
