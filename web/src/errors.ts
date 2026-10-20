import { ApiError } from './api';
import type { Diagnostic } from './generated/Diagnostic';
import type { DraftIssue } from './generated/DraftIssue';
import { plural } from './format';

/**
 * Server errors in plain words. The UI never shows the server's own text: every known code
 * has a sentence here, and an unknown code gets a generic one. The exceptions are a
 * host's reason (`host_unreachable`) and an agent's refusal of an answer (`answer_refused`), which are
 * runtime evidence and are kept verbatim.
 */

/** Why Stop is disabled while a team starts; also the words for a 409 `team_starting`. */
export const STARTING_REASON = 'Starting… you can stop the team once it has started';

const RELOAD = 'Reload the page and try again.';

/** Folder API errors (the folder picker). */
export const FOLDER_WORDS: Record<string, string> = {
  outside_home: 'Only folders inside your home folder can be chosen.',
  folder_missing: 'That folder does not exist any more.',
  not_a_folder: 'That is a file, not a folder.',
  folder_unreadable:
    "Sculptured can't read that folder. On a Mac, Desktop, Documents and Downloads need your permission: in System Settings → Privacy & Security, allow Sculptured under Files and Folders or Full Disk Access.",
  bad_path: "That folder path can't be used.",
  path_not_absolute: "That folder path can't be used.",
  bad_folder_name: "Folder names can't be empty, start with a dot, or contain '/' or invisible formatting characters.",
  folder_exists: 'A folder with that name is already here.',
  folder_unwritable: "Sculptured can't create a folder here.",
  folder_not_created: "Sculptured couldn't create that folder.",
  home_unknown: 'Your home folder is unknown, so folders cannot be browsed.',
};

/** Community's own answers (codes from its API and the gateway), in plain words. */
export const COMMUNITY_ERROR_WORDS: Record<string, string> = {
  handle_taken: 'Someone already has that handle. Choose another.',
  handle_invalid: 'A handle is 3 to 30 lowercase letters, digits or dashes, and starts and ends with a letter or digit.',
  handle_reserved: 'That handle is reserved. Choose another.',
  handle_too_soon: 'You can change your handle once every 30 days.',
  bio_too_long: 'Keep your bio to 280 characters.',
  link_invalid: 'Each link must start with https:// and be at most 200 characters. You can add up to 3.',
  avatar_too_large: 'That picture is too large. Use one under 256 KB.',
  avatar_type: 'Use a PNG, JPEG or WebP picture.',
  handle_required: 'Choose a handle before you publish.',
  publish_limit: 'A new account can publish 5 blocks in its first day. Try again tomorrow.',
  signed_out: 'Sign in to Sculptured to do this.',
  csrf: `Your sign-in could not be checked. ${RELOAD}`,
  community_bad_answer: 'Community answered in a way Sculptured does not accept. Try again later.',
  account_disabled: 'This Community account is disabled. Contact Sculptured support if you think this is a mistake.',
  rate_limited: 'Too many requests to Community just now. Wait a minute, then try again.',
  invalid: 'Community did not accept that. Check what you entered, then try again.',
  device_unknown: 'Your Community sign-in was removed. Sign in again.',
  community_unavailable: 'Community is not available on this Sculptured server right now.',
  // Between this computer and Community (controld's own answers, and the gateway's while signing in).
  community_off: 'Community is off on this computer. Turn it on to use it.',
  community_unreachable: 'Could not reach Community. Check your internet connection, then try again.',
  community_timeout: 'Community did not answer within 15 seconds. Try again.',
  community_too_large: 'That is too large to send to Community (the limit is 300 KB).',
  community_origin_invalid: 'Use an https:// address, such as https://app.usesculptured.com.',
  bad_scope: `Sculptured refused this kind of sign-in. ${RELOAD}`,
  too_many_attempts: 'Too many sign-in attempts. Wait a few minutes, then try again.',
};

export const ERROR_WORDS: Record<string, string> = {
  ...COMMUNITY_ERROR_WORDS,
  no_sign_in: 'That sign-in has ended. Press Sign in to start again.',
  bad_code: 'Paste the code exactly as the sign-in page shows it, on one line.',
  unknown_op: 'That computer runs an older Sculptured. Update it (run its connect command again), then try again.',
  ...FOLDER_WORDS,
  group_block_invalid: 'This group contains invalid or external references. Keep its agents and internal connections together before saving.',
  group_block_conflict: 'This saved group changed elsewhere. Reload it before saving your edits.',
  // The request envelope: the page sent something Sculptured could not take.
  bad_json: `Sculptured could not read the request this page sent. ${RELOAD}`,
  bad_body: `Sculptured did not accept the request this page sent; the page may be out of date. ${RELOAD}`,
  json_required: `This page sent its request in a form Sculptured does not accept. ${RELOAD}`,
  body_too_large: 'That is too large to send to Sculptured (the limit is 2 MB). Make it shorter and try again.',
  bad_query: `Sculptured did not understand what this page asked for. ${RELOAD}`,
  bad_url: `Sculptured could not read the address this page used. ${RELOAD}`,
  method_not_allowed: `Sculptured does not do that at this address; the page may be out of date. ${RELOAD}`,
  request_id_required: `Sculptured refused the request because it was not labelled. ${RELOAD}`,
  request_id_reused: `Sculptured refused a repeated request. ${RELOAD}`,
  team_id_mismatch: `This page and Sculptured disagree about which team this is. ${RELOAD}`,
  project_trashed: 'Restore this project from Trash before editing or starting it.',
  project_running: 'Stop this project before moving it to Trash. Hiding it leaves agents running.',
  // Teams.
  team_starting: STARTING_REASON,
  team_running: 'This team is running. Stop it first to add, remove or reconnect agents, or to start it again.',
  rig_running: 'This team is already running. Stop it before starting it again.',
  team_not_started: 'This team has not been started yet.',
  workflow_retry: 'This stage cannot be retried yet. Check that this is the current batch, the agent is running and idle, delivery has finished, and the step limit has not been reached.',
  textbook_workflow: 'Choose a workflow project for the textbook starter.',
  textbook_folder: 'Choose a dedicated empty project folder first.',
  textbook_not_empty: 'This folder already contains files. Choose a new empty folder; existing work is never replaced.',
  workflow_topic: 'Could not change topic execution. Refresh the current batch; only never-assigned topics can change. Dependent topics need existing prerequisites without cycles, and independent topics need none.',
  draft_conflict: 'This team was changed somewhere else. Load the latest version, or keep yours.',
  rig_name_taken:
    'Another team already uses this ID, and a team’s ID never changes. If the other team was started from a file, change the name inside that file and start it again.',
  unknown_template: 'That template is not available. Reload the page and pick another.',
  not_found: 'Sculptured could not find that; it may have been removed. Reload the page.',
  // Team files (import and the expert start-from-a-file form).
  spec_invalid: 'The team file has problems.',
  spec_unreadable: 'Sculptured could not read that team file. Check the path and that the file exists.',
  // The address guard: a page opened from another address (or another site) asked.
  forbidden_host:
    'Sculptured only answers pages opened at its own address on this computer. Open Sculptured from the address it printed when it started (for example http://127.0.0.1:4410).',
  // Questions, settings, sessions and restore (M2).
  question_closed: 'This question was already answered or withdrawn, so your answer was not sent.',
  bad_answer: 'That answer does not fit this question. Pick one of its choices or type an answer.',
  question_open: 'This question is still waiting for an answer. Answer it instead.',
  question_busy: 'Someone else is answering this question right now. Try again in a moment.',
  answer_refused: 'The agent did not take this answer.',
  answer_confirmed: 'The agent already has this answer, so there is nothing to send again.',
  reason_required: 'Say why. A reason is required and is kept with the agent’s history.',
  fork_unsupported: 'This agent cannot copy its session. Start a fresh session instead.',
  agent_not_running: 'The agent is not running, so that cannot be done now.',
  team_not_running: 'This team is not running. Start the team first.',
  no_restore_needed: 'No agent here waits for a decision about bringing it back any more. Reload the page.',
  restore_failed: 'The agent could not be brought back. Its reason is shown on the agent.',
  restart_failed: 'The agent could not be started again. Its reason is shown on the agent.',
  cancel_timed_out: 'The start did not stop within 15 seconds. The stop is recorded; press Cancel start again to stop the agents that are still starting.',
  restore_pending: 'Sculptured restarted while this team was running. Answer "Resume N agents?" in Needs you first.',
  bad_settings: 'Those settings are out of range. The question timeout is 1 to 1440 minutes.',
  needs_computer: 'This needs your computer: your folders and agents run there, not on the server. Open Computers and connect your computer, then try again.',
  older_runner:
    "Sculptured's agent runner is from an older version. Stop all teams, then end the old agent runner: its process number is in the file hostd.pid, in the host-local folder of Sculptured's data folder. Sculptured starts the new runner within a minute.",
  // Everything else.
  internal: 'Sculptured hit an internal problem. Try again; if it keeps happening, restart Sculptured.',
  login_required: 'Sign in to use Sculptured.',
  unauthorized: 'Sculptured refused this request.',
};

/** The diagnostics an API error carries (a team file's problems), or none. */
export const diagnosticsOf = (e: unknown): Diagnostic[] => (e instanceof ApiError ? (e.diagnostics ?? []) : []);

/** The draft issues an API error carries (a team that cannot start), or none. */
export const issuesOf = (e: unknown): DraftIssue[] => (e instanceof ApiError ? (e.issues ?? []) : []);

export const UNREACHABLE = 'Could not reach Sculptured. Check that it is still running, then try again.';

/** One sentence for any error from the API client. */
// Start refusals name the plan, the computer or the project in the server's words; generic words would hide why.
const VERBATIM = new Set(['plan_limit', 'needs_computer', 'project_archived', 'generate_unusable', 'generate_failed', 'generate_invalid', 'generate_timeout', 'design_too_large', 'proposal_invalid', 'no_request', 'cli_not_signed_in', 'cli_missing', 'not_allowed_here', 'computer_timeout', 'session_file_invalid', 'runner_without_reports', 'no_logs', 'no_sessions']);

export function errorText(e: unknown): string {
  if (!(e instanceof ApiError)) return UNREACHABLE;
  if (e.code === 'team_invalid') {
    const n = (e.issues ?? []).filter((i) => i.severity === 'error').length || (e.issues ?? []).length;
    return n > 0 ? `${plural(n, 'thing')} to fix` : 'This team has things to fix before it can start.';
  }
  // A host's reason is runtime evidence: kept verbatim.
  if (e.code === 'host_unreachable') return `Sculptured could not reach this agent's host: ${e.message}`;
  // So is the agent's refusal of an answer (its program's words).
  if (e.code === 'answer_refused') return `${ERROR_WORDS.answer_refused.slice(0, -1)}: ${e.message}`;
  // The generator's and the user's computer's answers are already in plain words: kept verbatim.
  if (VERBATIM.has(e.code) && e.message) return e.message;
  if (e.code === 'http_error') return `Sculptured answered in an unexpected way (error ${e.status}). ${RELOAD}`;
  return ERROR_WORDS[e.code] ?? `Sculptured could not do that (error ${e.status}). ${RELOAD}`;
}
