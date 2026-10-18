import type { Box } from '../layout';
import type { CompiledEdge } from '../generated/CompiledEdge';

type Point = { x: number; y: number };
/** `room`: the free width around the label (between two side-by-side boxes), or Infinity. */
export type EdgeGeom = { d: string; mid: Point; room: number; pts: [Point, Point, Point, Point] };

/** Point on a cubic Bézier at t = 0.5. */
function mid(p0: Point, p1: Point, p2: Point, p3: Point): Point {
  return { x: (p0.x + 3 * p1.x + 3 * p2.x + p3.x) / 8, y: (p0.y + 3 * p1.y + 3 * p2.y + p3.y) / 8 };
}

/**
 * A curved connection between two boxes. Rightward edges leave the right side and enter the
 * left side; vertical neighbours use bottom and top; anything else arcs below both boxes so a
 * return connection never sits on top of the forward one.
 */
export function edgeGeometry(a: Box, b: Box, vertical = false): EdgeGeom {
  let p0: Point, p1: Point, p2: Point, p3: Point;
  let room = Infinity;
  if (vertical && b.y >= a.y + a.h) {
    p0 = {x:a.x+a.w/2,y:a.y+a.h};
    p3 = {x:b.x+b.w/2,y:b.y};
    p1 = {x:p0.x,y:(p0.y+p3.y)/2};
    p2 = {x:p3.x,y:(p0.y+p3.y)/2};
  } else if (b.x >= a.x + a.w) {
    p0 = { x: a.x + a.w, y: a.y + a.h / 2 };
    p3 = { x: b.x, y: b.y + b.h / 2 };
    const dx = Math.max(40, (p3.x - p0.x) / 2);
    room = p3.x - p0.x;
    p1 = { x: p0.x + dx, y: p0.y };
    p2 = { x: p3.x - dx, y: p3.y };
  } else if (b.y >= a.y + a.h) {
    p0 = { x: a.x + a.w / 2, y: a.y + a.h };
    p3 = { x: b.x + b.w / 2, y: b.y };
    const dy = Math.max(30, (p3.y - p0.y) / 2);
    p1 = { x: p0.x, y: p0.y + dy };
    p2 = { x: p3.x, y: p3.y - dy };
  } else if (b.y + b.h <= a.y) {
    p0 = { x: a.x + a.w / 2, y: a.y };
    p3 = { x: b.x + b.w / 2, y: b.y + b.h };
    const dy = Math.max(30, (p0.y - p3.y) / 2);
    p1 = { x: p0.x, y: p0.y - dy };
    p2 = { x: p3.x, y: p3.y + dy };
  } else {
    p0 = { x: a.x + a.w / 2 + 24, y: a.y + a.h };
    p3 = { x: b.x + b.w / 2 - 24, y: b.y + b.h };
    const low = Math.max(p0.y, p3.y) + 70;
    p1 = { x: p0.x, y: low };
    p2 = { x: p3.x, y: low };
  }
  const f = (n: number) => Math.round(n * 10) / 10;
  return { d: `M${f(p0.x)},${f(p0.y)} C${f(p1.x)},${f(p1.y)} ${f(p2.x)},${f(p2.y)} ${f(p3.x)},${f(p3.y)}`, mid: mid(p0, p1, p2, p3), room, pts: [p0, p1, p2, p3] };
}

const overlaps = (a0: number, a1: number, b0: number, b1: number) => a0 < b1 && b0 < a1;
/** Around an arrow's own ends, other arrows from the same port are expected, not crowding. */
const PORT_RADIUS = 32;
const far = (p: Point, q: Point) => (p.x - q.x) ** 2 + (p.y - q.y) ** 2 > PORT_RADIUS ** 2;

/** Points of the arrows already drawn, in 9 px cells, so "near another arrow" only looks next door. */
class ArrowGrid {
  private cells = new Map<string, { x: number; y: number; edge: number }[]>();
  private count = 0;

  add(points: Point[]) {
    for (const q of points) {
      const k = `${Math.floor(q.x / 9)},${Math.floor(q.y / 9)}`;
      let list = this.cells.get(k);
      if (!list) this.cells.set(k, (list = []));
      list.push({ ...q, edge: this.count });
    }
    this.count++;
  }

  /** How many drawn arrows pass within 9 px of `p`. */
  near(p: Point): number {
    const hits = new Set<number>();
    const cx = Math.floor(p.x / 9), cy = Math.floor(p.y / 9);
    for (let dx = -1; dx <= 1; dx++)
      for (let dy = -1; dy <= 1; dy++)
        for (const q of this.cells.get(`${cx + dx},${cy + dy}`) ?? []) if (Math.hypot(p.x - q.x, p.y - q.y) < 9) hits.add(q.edge);
    return hits.size;
  }
}

/**
 * Geometry for every connection. A rightward connection that would pass through another box
 * arcs over the top instead, each one a little higher, so no line or label sits on a box.
 */
export function routeEdges(edges: CompiledEdge[], seats: Record<string, Box>): Map<CompiledEdge, EdgeGeom> {
  const out = new Map<CompiledEdge, EdgeGeom>();
  const boxes = Object.entries(seats);
  let over = 0;
  const ordered=[...edges].sort((a,b)=>`${a.from}:${a.to}:${a.kind}`.localeCompare(`${b.from}:${b.to}:${b.kind}`));
  const curve=(pts:EdgeGeom['pts']):EdgeGeom=>({pts,d:`M${pts[0].x},${pts[0].y} C${pts[1].x},${pts[1].y} ${pts[2].x},${pts[2].y} ${pts[3].x},${pts[3].y}`,mid:mid(...pts),room:Infinity});
  const sample=(g:EdgeGeom)=>Array.from({length:49},(_,i)=>{const t=i/48,u=1-t,[a,b,c,d]=g.pts;return{x:u*u*u*a.x+3*u*u*t*b.x+3*u*t*t*c.x+t*t*t*d.x,y:u*u*u*a.y+3*u*u*t*b.y+3*u*t*t*c.y+t*t*t*d.y}});
  const drawn=new ArrowGrid();
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
          // Arrows that share a port (a planner handing work to two builders) meet there by design:
          // company counts only away from this arrow's own ends, or the fan would loop round the cards.
          if(far(p,start)&&far(p,end))n+=12*drawn.near(p);
        }
        return n;
      };
      let best=Infinity;for(const candidate of candidates){const c=cost(candidate);if(c<best){best=c;g=candidate;}}
    }
    drawn.add(sample(g));
    out.set(e, g);
  }
  return out;
}

