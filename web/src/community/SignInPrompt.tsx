/** Why signing in is needed here, with the way to do it. */
export function SignInPrompt({ why }: { why: string }) {
  return <p className="cm-signin">{why} <a href="/login">Sign in</a></p>;
}
