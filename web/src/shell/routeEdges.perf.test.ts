import { describe, expect, it } from 'vitest';
import type { Box } from '../layout';
import type { CompiledEdge } from '../generated/CompiledEdge';
import { edgeGeometry, routeEdges } from './edgeGeometry';

type Point = { x: number; y: number };
type EdgeGeom = ReturnType<typeof edgeGeometry>;
function mid(p0: Point, p1: Point, p2: Point, p3: Point): Point {
  return { x: (p0.x + 3 * p1.x + 3 * p2.x + p3.x) / 8, y: (p0.y + 3 * p1.y + 3 * p2.y + p3.y) / 8 };
}
const overlaps = (a0: number, a1: number, b0: number, b1: number) => a0 < b1 && b0 < a1;

/** The routing before the spatial grid, kept here to prove the fast version draws the same arrows. */
export function referenceRouteEdges(edges: CompiledEdge[], seats: Record<string, Box>): Map<CompiledEdge, EdgeGeom> {
  const out = new Map<CompiledEdge, EdgeGeom>();
  const boxes = Object.entries(seats);
  let over = 0;
  const ordered=[...edges].sort((a,b)=>`${a.from}:${a.to}:${a.kind}`.localeCompare(`${b.from}:${b.to}:${b.kind}`));
  const curve=(pts:EdgeGeom['pts']):EdgeGeom=>({pts,d:`M${pts[0].x},${pts[0].y} C${pts[1].x},${pts[1].y} ${pts[2].x},${pts[2].y} ${pts[3].x},${pts[3].y}`,mid:mid(...pts),room:Infinity});
  const sample=(g:EdgeGeom)=>Array.from({length:49},(_,i)=>{const t=i/48,u=1-t,[a,b,c,d]=g.pts;return{x:u*u*u*a.x+3*u*u*t*b.x+3*u*t*t*c.x+t*t*t*d.x,y:u*u*u*a.y+3*u*u*t*b.y+3*u*t*t*c.y+t*t*t*d.y}});
  const drawn:Point[][]=[];
  for (const e of ordered) {
    const a = seats[e.from];
    const b = seats[e.to];
    if (!a || !b) continue;
    let g = edgeGeometry(a, b, e.kind === 'spawned_by');
    if (e.kind !== 'spawned_by' && b.x >= a.x + a.w) {
      const y0 = Math.min(a.y + a.h / 2, b.y + b.h / 2) - 1;
      const y1 = Math.max(a.y + a.h / 2, b.y + b.h / 2) + 1;
      const blocked = boxes.some(
        ([id, c]) => id !== e.from && id !== e.to && overlaps(a.x + a.w, b.x, c.x, c.x + c.w) && overlaps(y0, y1, c.y, c.y + c.h),
      );
      if (blocked) {
        const top = Math.min(...boxes.filter(([, c]) => overlaps(a.x, b.x + b.w, c.x, c.x + c.w)).map(([, c]) => c.y));
        const apex = top - 96 - 28 * over++;
        const p0 = { x: a.x + a.w * 0.6, y: a.y };
        const p3 = { x: b.x + b.w * 0.5, y: b.y };
        const p1 = { x: p0.x, y: apex };
        const p2 = { x: p3.x, y: apex };
        const f = (n: number) => Math.round(n * 10) / 10;
        g = { d: `M${f(p0.x)},${f(p0.y)} C${f(p1.x)},${f(p1.y)} ${f(p2.x)},${f(p2.y)} ${f(p3.x)},${f(p3.y)}`, mid: mid(p0, p1, p2, p3), room: Infinity, pts: [p0, p1, p2, p3] };
      }
    }
    if(e.kind==='spawned_by' && b.y>=a.y+a.h) {
      // Separate ports in child order, so a wide family does not overprint at its source.
      const siblings=ordered.filter(s=>s.kind==='spawned_by'&&s.from===e.from&&seats[s.to]).sort((u,v)=>seats[u.to].x-seats[v.to].x||u.to.localeCompare(v.to));
      const x=a.x+a.w*(siblings.indexOf(e)+1)/(siblings.length+1);
      const [p0,p1,p2,p3]=g.pts;g=curve([{...p0,x},{...p1,x},p2,p3]);
    } else if(e.kind!=='spawned_by') {
      const parallel=ordered.filter(s=>s.from===e.from&&s.to===e.to);
      const lane=parallel.indexOf(e),pad=80+lane*32;
      const reciprocal=edges.some(s=>s.from===e.to&&s.to===e.from);
      const upper=Math.min(a.y,b.y)-pad,lower=Math.max(a.y+a.h,b.y+b.h)+pad;
      const left=Math.min(a.x,b.x)-pad,right=Math.max(a.x+a.w,b.x+b.w)+pad;
      const top=curve([{x:a.x+a.w*.6,y:a.y},{x:a.x+a.w*.6,y:upper},{x:b.x+b.w*.4,y:upper},{x:b.x+b.w*.4,y:b.y}]);
      const bottom=curve([{x:a.x+a.w*.4,y:a.y+a.h},{x:a.x+a.w*.4,y:lower},{x:b.x+b.w*.6,y:lower},{x:b.x+b.w*.6,y:b.y+b.h}]);
      const west=curve([{x:a.x,y:a.y+a.h*.4},{x:left,y:a.y+a.h*.4},{x:left,y:b.y+b.h*.6},{x:b.x,y:b.y+b.h*.6}]);
      const east=curve([{x:a.x+a.w,y:a.y+a.h*.6},{x:right,y:a.y+a.h*.6},{x:right,y:b.y+b.h*.4},{x:b.x+b.w,y:b.y+b.h*.4}]);
      const loop=Math.abs(b.x-a.x)>=Math.abs(b.y-a.y)?(b.x>=a.x?top:bottom):(b.y>=a.y?east:west);
      const candidates=reciprocal||parallel.length>1||e.from===e.to?[loop,top,bottom,west,east,g]:[g,top,bottom,west,east];
      const cost=(candidate:EdgeGeom)=>{
        const points=sample(candidate);let n=0;
        const [start,,,end]=candidate.pts;
        for(const p of points.slice(1,-1)) {
          for(const [id,c] of boxes){const margin=id===e.from||id===e.to?0:10;if(p.x>c.x-margin&&p.x<c.x+c.w+margin&&p.y>c.y-margin&&p.y<c.y+c.h+margin)n+=10000;}
          // Other arrows sharing a port are not crowding near this arrow's own ends.
          if(Math.hypot(p.x-start.x,p.y-start.y)<=32||Math.hypot(p.x-end.x,p.y-end.y)<=32)continue;
          for(const previous of drawn)if(previous.some(q=>Math.hypot(p.x-q.x,p.y-q.y)<9))n+=12;
        }
        return n;
      };
      let best=Infinity;for(const candidate of candidates){const c=cost(candidate);if(c<best){best=c;g=candidate;}}
    }
    drawn.push(sample(g));
    out.set(e, g);
  }
  return out;
}



/** A seeded team: `n` cards on a loose grid and `m` arrows among them. */
function team(n: number, m: number, seed: number) {
  let s = seed;
  const rand = () => ((s = (s * 1103515245 + 12345) % 2 ** 31) / 2 ** 31);
  const seats: Record<string, Box> = {};
  for (let i = 0; i < n; i++) seats[`s${i}`] = { x: (i % 6) * 260 + Math.floor(rand() * 40), y: Math.floor(i / 6) * 200 + Math.floor(rand() * 40), w: 200, h: 120 };
  const kinds = ['delegates_to', 'collaborates_with', 'spawned_by'] as const;
  const edges: CompiledEdge[] = Array.from({ length: m }, () => ({ from: `s${Math.floor(rand() * n)}`, to: `s${Math.floor(rand() * n)}`, kind: kinds[Math.floor(rand() * 3)], enforced: true }) as CompiledEdge);
  return { seats, edges };
}

describe('routeEdges with a spatial grid', () => {
  it('draws exactly the arrows the full comparison drew', () => {
    for (const seed of [1, 7, 42, 99, 2026]) {
      const { seats, edges } = team(26, 56, seed);
      const fast = routeEdges(edges, seats), slow = referenceRouteEdges(edges, seats);
      for (const e of edges) expect(fast.get(e)?.d, `seed ${seed}`).toBe(slow.get(e)?.d);
    }
  });

  it('routes a 26-agent, 56-arrow team in well under a frame budget', () => {
    const { seats, edges } = team(26, 56, 3);
    const t0 = performance.now();
    for (let i = 0; i < 5; i++) routeEdges(edges, seats);
    expect((performance.now() - t0) / 5).toBeLessThan(40);
  });
});
