import { CARD_W, CARD_H, COL_GAP, POD_PAD, POD_HEAD, type Layout, type Box } from '../layout';
type TreeTopology = {seats: {id:string;pod:string}[];pods:{id:string}[]};

export type ParentLink = { parent: string; child: string };

/** Validate the observed forest. Unknown endpoints and cyclic/duplicate parents stay unlinked. */
export function parentForest(rig: TreeTopology, links: ParentLink[]): ParentLink[] {
  const ids = new Set(rig.seats.map(s => s.id));
  const parents = new Map<string, string>();
  for (const {parent, child} of links) {
    if (!ids.has(parent) || !ids.has(child) || parent === child || parents.has(child)) continue;
    let ancestor: string | undefined = parent;
    while (ancestor && ancestor !== child) ancestor = parents.get(ancestor);
    if (ancestor === child) continue;
    parents.set(child, parent);
  }
  return [...parents].map(([child, parent]) => ({parent, child}));
}

/** Tidy top-down forest. Only topology changes placement; activity, messages and visibility do not. */
export function treeLayout(saved: Layout, rig: TreeTopology, links: ParentLink[]): Layout {
  if (!links.length) return saved;
  const children = new Map<string, string[]>();
  const parents = new Set(links.map(l => l.child));
  const order = new Map(rig.seats.map((s, i) => [s.id, i]));
  for (const link of links) children.set(link.parent, [...(children.get(link.parent) ?? []), link.child]);
  for (const ids of children.values()) ids.sort((a,b) => order.get(a)! - order.get(b)!);
  const widths = new Map<string, number>();
  const gap = 80;
  const measure = (id: string): number => {
    const kids = children.get(id) ?? [];
    const width = Math.max(CARD_W, kids.reduce((sum,k) => sum + measure(k), 0) + Math.max(0,kids.length-1)*gap);
    widths.set(id,width);
    return width;
  };
  const seats: Record<string, Box> = {};
  const place = (id: string, x: number, depth: number) => {
    const width = widths.get(id)!;
    seats[id] = {x:x+(width-CARD_W)/2, y:POD_HEAD+POD_PAD+depth*(CARD_H+COL_GAP), w:CARD_W, h:CARD_H};
    let cursor = x;
    for (const child of children.get(id) ?? []) {
      place(child,cursor,depth+1);
      cursor += widths.get(child)!+gap;
    }
  };
  let x = POD_PAD;
  for (const root of rig.seats.filter(s => !parents.has(s.id))) {
    measure(root.id);
    place(root.id,x,0);
    x += widths.get(root.id)!+COL_GAP;
  }
  const pods = groupBounds(rig, seats);
  return {seats,pods,width:Math.max(0,...Object.values(pods).map(b=>b.x+b.w)),height:Math.max(0,...Object.values(pods).map(b=>b.y+b.h))};
}

function groupBounds(rig: TreeTopology, seats: Record<string, Box>): Layout['pods'] {
  const pods: Layout['pods'] = {};
  for (const pod of rig.pods) {
    const boxes = rig.seats.filter(s=>s.pod===pod.id).map(s=>seats[s.id]);
    if (!boxes.length) continue;
    const left=Math.min(...boxes.map(b=>b.x))-POD_PAD;
    const top=Math.min(...boxes.map(b=>b.y))-POD_PAD-POD_HEAD;
    pods[pod.id]={x:left,y:top,w:Math.max(...boxes.map(b=>b.x+b.w))-left+POD_PAD,h:Math.max(...boxes.map(b=>b.y+b.h))-top+POD_PAD};
  }
  return pods;
}

/** Inclusive descendants, including stopped ancestors so live grandchildren remain reachable. */
export function subtreeIds(rig: TreeTopology, links: ParentLink[], root: string): Set<string> {
  const found = new Set<string>();
  if (!rig.seats.some(s=>s.id===root)) return found;
  found.add(root);
  const family = parentForest(rig,links);
  const queue=[root];
  for(let i=0;i<queue.length;i++) for(const link of family) {
    if(link.parent===queue[i] && !found.has(link.child)){found.add(link.child);queue.push(link.child);}
  }
  return found;
}
