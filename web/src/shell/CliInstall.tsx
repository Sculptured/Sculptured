import { useState } from 'react';

/** Each CLI's own installer, as Anthropic and OpenAI publish it. Both put the program in
 * `~/.local/bin`, where the runner looks, so it is found without restarting anything. */
const INSTALL: Record<string, { unix: string; windows: string }> = {
  claude: { unix: 'curl -fsSL https://claude.ai/install.sh | bash', windows: 'irm https://claude.ai/install.ps1 | iex' },
  codex: { unix: 'curl -fsSL https://chatgpt.com/codex/install.sh | sh', windows: 'irm https://chatgpt.com/codex/install.ps1 | iex' },
};

/** How to install a missing CLI on the computer that needs it: one command to paste there. */
export function CliInstall({ cli }: { cli: string }) {
  const cmds = INSTALL[cli];
  if (!cmds) return null;
  return (
    <div className="cli-install">
      <p className="muted">Install it on that computer: paste one command into its Terminal (PowerShell on Windows), then sign in here.</p>
      <Command label="Mac or Linux" text={cmds.unix} />
      <Command label="Windows" text={cmds.windows} />
    </div>
  );
}

function Command({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => void navigator.clipboard?.writeText(text).then(() => setCopied(true), () => setCopied(false));
  return (
    <div role="group" aria-label={label} className="cli-install-cmd">
      <span className="muted">{label}:</span> <code>{text}</code>{' '}
      <button type="button" className="btn btn-sm" onClick={copy} aria-label="Copy">{copied ? 'Copied' : 'Copy'}</button>
    </div>
  );
}
