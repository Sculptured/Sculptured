import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import type { Item } from '../conversation';
import { scrollWithin } from './highlight';
import { Icon } from './ui';
import { isTextField, type Scroller, type Stick } from './transcriptScroll';

/** The search field and its matches: counted from the marks drawn, with the current one brought into view. */
export function useTranscriptSearch(scroller: Scroller, items: Item[], stick: Stick, toolsSlot: HTMLElement | null | undefined) {
  const [query, setQuery] = useState('');
  const [hit, setHit] = useState(0);
  const [hits, setHits] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  // In the header the field folds to an icon until it is used; `focusSearch` focuses it once drawn.
  const [searchOpen, setSearchOpen] = useState(false);
  const focusSearch = useRef(false);
  const fieldShown = !toolsSlot || searchOpen || query !== '';
  const q = query.trim().toLowerCase();
  useEffect(() => {
    if (focusSearch.current && fieldShown) {
      focusSearch.current = false;
      searchRef.current?.focus();
      searchRef.current?.select();
    }
  });
  const openSearch = () => {
    focusSearch.current = true;
    setSearchOpen(true);
  };
  useLayoutEffect(() => {
    const marks = q ? [...(scroller.current?.querySelectorAll<HTMLElement>('mark.hit') ?? [])] : [];
    setHits(marks.length);
    const i = marks.length ? Math.min(hit, marks.length - 1) : 0;
    marks.forEach((m, j) => m.classList.toggle('current', j === i));
    if (marks[i]) {
      stick.atBottom.current = false;
      scrollWithin(scroller.current, marks[i]);
    }
  }, [q, hit, items, scroller, stick]);
  const step = useCallback((d: number) => setHit((h) => (hits ? (Math.min(h, hits - 1) + d + hits) % hits : 0)), [hits]);
  return { query, setQuery, q, hit, setHit, hits, searchRef, fieldShown, setSearchOpen, openSearch, step };
}

export type Search = ReturnType<typeof useTranscriptSearch>;

/** `/` searches the transcript; Esc goes back to where you came from. */
export function useTranscriptKeys(search: Search, onReturn: (() => void) | undefined) {
  const { searchRef, openSearch } = search;
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || isTextField(e.target)) return;
      if (document.querySelector('[role="dialog"]')) return;
      if (e.key === '/') {
        e.preventDefault();
        if (searchRef.current) {
          searchRef.current.focus();
          searchRef.current.select();
        } else openSearch();
      } else if (e.key === 'Escape' && onReturn) {
        e.preventDefault();
        onReturn();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onReturn]);
}


function SearchNav({ search }: { search: Search }) {
  const { hit, hits, step, setQuery } = search;
  return (
    <span className="cv-search-nav">
      <span className="cv-count" role="status">
        {hits ? `${Math.min(hit, hits - 1) + 1} of ${hits}` : 'No matches'}
      </span>
      <button type="button" className="icon-btn" onClick={() => step(-1)} disabled={!hits} aria-label="Previous match" data-tip="Previous match" data-kbd="⇧⏎">
        <Icon name="arrow-up" size={13} />
      </button>
      <button type="button" className="icon-btn flip" onClick={() => step(1)} disabled={!hits} aria-label="Next match" data-tip="Next match" data-kbd="⏎">
        <Icon name="arrow-up" size={13} />
      </button>
      <button type="button" className="icon-btn" onClick={() => setQuery('')} aria-label="Clear search" data-tip="Clear search">
        <Icon name="x" size={13} />
      </button>
    </span>
  );
}

function SearchField({ search }: { search: Search }) {
  const { query, setQuery, setHit, searchRef, setSearchOpen, step } = search;
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (query) setQuery('');
      else e.currentTarget.blur();
    }
  };
  return (
    <label className="cv-search">
      <Icon name="search" size={13} className="muted" />
      <input
        ref={searchRef}
        type="search"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setHit(0);
        }}
        onKeyDown={onKey}
        onBlur={() => { if (!query) setSearchOpen(false); }}
        placeholder="Search transcript"
        aria-label="Search transcript"
        aria-keyshortcuts="/"
      />
      {!query && (
        <kbd className="kbd" aria-hidden="true">
          /
        </kbd>
      )}
    </label>
  );
}

export function SearchTools({ search, onReturn, returnLabel }: { search: Search; onReturn?: () => void; returnLabel?: string }) {
  return (
    <>
      {onReturn && (
        <button type="button" className="return-btn" onClick={onReturn} aria-keyshortcuts="Escape" data-tip="Go back" data-kbd="esc">
          <Icon name="arrow-left" size={13} />
          {returnLabel ?? 'Back'}
        </button>
      )}
      {!search.fieldShown && (
        <button type="button" className="icon-btn cv-search-btn" onClick={search.openSearch} aria-label="Search transcript" aria-keyshortcuts="/" data-tip="Search transcript" data-kbd="/">
          <Icon name="search" size={15} />
        </button>
      )}
      {search.fieldShown && <SearchField search={search} />}
      {search.q && <SearchNav search={search} />}
    </>
  );
}

