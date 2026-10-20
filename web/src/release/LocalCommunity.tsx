import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { communityLink } from "./workspaceLink";
import type { Settings } from "../generated/Settings";
import { Icon } from "../shell/ui";

/** "https://community.example.com/x" → "community.example.com"; anything unparsable as given. */
const hostOf = (url: string) => { try { return new URL(url).host; } catch { return url; } };

/** The local account entry; provider cookies remain on the configured service origin. */
export function LocalCommunity() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [address, setAddress] = useState("");
  const [notice, setNotice] = useState("");
  const [changing, setChanging] = useState(false);
  useEffect(() => {
    api.settings().then(
      (value) => {
        setSettings(value);
        setAddress(value.sculptured_origin ?? "");
      },
      () => setNotice("Could not load your community connection."),
    );
  }, []);
  const savedAddress = settings?.sculptured_origin;
  return (
    <section className="release-page community-account">
      <h1>Community</h1>
      <p>Your account, saved blocks, and publications. New to composing agents? <Link to="/community/learn">Learn how to design a team</Link>.</p>
      <div className="release-card connect-card">
        <div className="connect-head">
          <span className="connect-mark" aria-hidden="true"><Icon name="globe" size={20} /></span>
          <div>
            <h2>{savedAddress ? "Your community account" : "Connect your account"}</h2>
            <p className="muted">
              {savedAddress
                ? "Sign in to manage your profile and shared blocks."
                : "Enter your community service address to sign in. This workspace keeps running here."}
            </p>
          </div>
        </div>
        {savedAddress && (
          <div className="connect-actions">
            <a className="btn btn-primary" href={communityLink(savedAddress, "")}>Open your account →</a>
            <a className="btn" href={communityLink(savedAddress, "/explore")}>Browse blocks</a>
            <a className="btn" href={communityLink(savedAddress, "/saved")}>Saved blocks</a>
          </div>
        )}
        {savedAddress && !changing ? (
          <div className="connect-saved">
            <span className="connect-dot" aria-hidden="true" />
            <span className="muted">Connected to</span>
            <code>{hostOf(savedAddress)}</code>
            <button type="button" className="btn" onClick={() => setChanging(true)}>Change address</button>
          </div>
        ) : (
          <form
            className="release-form connect-form"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!settings) return;
              try {
                const next = { ...settings, sculptured_origin: address.trim() || undefined };
                await api.saveSettings(next);
                setSettings(next);
                setChanging(false);
                setNotice("Community connection saved.");
              } catch {
                setNotice("Could not save. Use an HTTPS service address, or localhost for development.");
              }
            }}
          >
            <label htmlFor="community-address">Service address</label>
            <div className="connect-row">
              <input id="community-address" type="url" placeholder="https://community.usesculptured.com" value={address} onChange={(e) => setAddress(e.target.value)} />
              <button className="btn btn-primary" disabled={!settings}>Save connection</button>
            </div>
          </form>
        )}
      </div>
      <p>
        Your <Link to="/library/groups">private Library</Link> stays on this
        machine. Export and review a group before publishing; choose Add to project on any saved or browsed block to open it here.
      </p>
      <p role="status">{notice}</p>
    </section>
  );
}
