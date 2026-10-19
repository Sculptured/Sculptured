const OPENRIG = /\b(rigs?|pods?|seats?|members?|edges?|runtimes?|rigspec|yaml|agent_ref|profiles?)\b/gi;
const PRODUCT = /Sculptured/g;

/**
 * OpenRig words a user would see outside an Advanced disclosure. Raw events
 * ([data-raw-evidence]) are evidence and are shown verbatim, so they are skipped too.
 */
/** Words of the underlying runtime in `root`'s visible text; `allow` lists ones a page may use in plain sense. */
export function openRigWords(root: HTMLElement, allow: readonly string[] = []): string[] {
  const clone = root.cloneNode(true) as HTMLElement;
  clone.querySelectorAll('[data-advanced], [data-raw-evidence]').forEach((n) => n.remove());
  const texts: string[] = [];
  const walker = document.createTreeWalker(clone, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) texts.push(n.textContent ?? '');
  clone.querySelectorAll('[aria-label], [title], [placeholder]').forEach((el) => {
    for (const a of ['aria-label', 'title', 'placeholder']) {
        // A title that carries verbatim runtime text is marked on its own element.
        if (a === 'title' && el.hasAttribute('data-raw-evidence-title')) continue;
        texts.push(el.getAttribute(a) ?? '');
      }
  });
  return texts.flatMap((t) => t.replace(PRODUCT, '').match(OPENRIG) ?? []).map((w) => w.toLowerCase()).filter((w) => !allow.includes(w));
}
