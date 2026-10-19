import type { ReactNode } from 'react';
import '@fontsource/roboto/latin-500.css';
import { DOCS_URL } from '../paths';
import { BrandMark } from '../shell/BrandMark';
import { providerHref } from './session';

export type Providers = { password: boolean; google: boolean; github: boolean };

/** The signed-out pages' frame: the brand, one card, and the links every visitor may want. */
export function AuthCard({ title, lead, children }: { title: string; lead?: ReactNode; children: ReactNode }) {
  return (
    <main className="auth-page">
      <div className="auth-column">
        <a className="auth-brand" href="/"><BrandMark /><span>Sculptured</span></a>
        <section className="auth-card" aria-labelledby="auth-title">
          <h1 id="auth-title">{title}</h1>
          {lead && <p className="auth-lead">{lead}</p>}
          {children}
        </section>
        <footer className="auth-footer">
          <a href={DOCS_URL} target="_blank" rel="noreferrer">Docs</a>
          <a href="/what-we-record" target="_blank" rel="noreferrer">Everything we record</a>
        </footer>
      </div>
    </main>
  );
}

/** Google's and GitHub's buttons, drawn to their own brand guidelines, then "or" when a form follows. */
export function ProviderButtons({ providers, purpose }: { providers: Providers | null; purpose: { join: string } | 'login' }) {
  if (!providers?.google && !providers?.github) return null;
  return (
    <>
      <div className="auth-providers">
        {providers.google && (
          <a className="gsi-button" href={providerHref(purpose, 'google')}><GoogleG /><span>Continue with Google</span></a>
        )}
        {providers.github && (
          <a className="github-button" href={providerHref(purpose, 'github')}><GitHubMark /><span>Continue with GitHub</span></a>
        )}
      </div>
      {providers.password && <p className="auth-divider"><span>or</span></p>}
    </>
  );
}

/** Google's standard "G" logo, unaltered (Sign in with Google branding guidelines). */
function GoogleG() {
  return (
    <svg aria-hidden="true" width="20" height="20" viewBox="0 0 48 48">
      <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" />
      <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" />
      <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" />
      <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" />
    </svg>
  );
}

/** GitHub's Invertocat mark (Primer Octicons, mark-github), unaltered. */
function GitHubMark() {
  return (
    <svg aria-hidden="true" width="20" height="20" viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z" />
    </svg>
  );
}
