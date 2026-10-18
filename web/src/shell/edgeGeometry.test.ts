import {it,expect} from 'vitest';
import {routeEdges} from './edgeGeometry';
import {bezierPoints} from './labels';
import {logoRouteBoxes} from './boardStyle';
it('uses separate ports for ten children and preserves routes when event order changes',()=>{
 const seats=Object.fromEntries(['root',...Array.from({length:10},(_,i)=>String(i))].map((id,i)=>[id,{x:i===0?1200:(i-1)*280,y:i===0?0:320,w:200,h:200}]));
 const edges=Array.from({length:10},(_,i)=>({from:'root',to:String(i),kind:'spawned_by' as const,enforced:true}));
 const routes=routeEdges(edges,seats);expect(new Set([...routes.values()].map(g=>g.pts[0].x)).size).toBe(10);
 const reversed=routeEdges([...edges].reverse(),seats);for(const e of edges)expect(routes.get(e)).toEqual(reversed.get(e));
});
it('separates reciprocal vertical arrows and routes around intervening cards',()=>{
 const seats={a:{x:0,y:0,w:200,h:200},b:{x:0,y:640,w:200,h:200},c:{x:0,y:320,w:200,h:200}};
 const edges=[{from:'a',to:'b',kind:'delegates_to' as const,enforced:true},{from:'b',to:'a',kind:'delegates_to' as const,enforced:true}];
 const g=routeEdges(edges,seats);expect(g.get(edges[0])?.mid.x).not.toBe(g.get(edges[1])?.mid.x);
 for(const route of g.values())expect(bezierPoints(route.pts).some(p=>p.x>0&&p.x<200&&p.y>320&&p.y<520)).toBe(false);
});
// The "Planner → 2 Builders" template: Builder 1 up and right, Builder 2 down and right.
const planner = {planner:{x:40,y:160,w:200,h:200},b1:{x:300,y:40,w:200,h:200},b2:{x:300,y:280,w:200,h:200}};
const fan = [{from:'planner',to:'b1',kind:'delegates_to' as const,enforced:true},{from:'planner',to:'b2',kind:'delegates_to' as const,enforced:true}];
it('fans a planner out port to port to builders up and down on its right, as the template lays them out',()=>{
 for(const order of [fan,[...fan].reverse()]){
  const routes=routeEdges(order,planner);
  for(const e of fan){
   const g=routes.get(e)!,a=planner.planner,b=planner[e.to as 'b1'|'b2'];
   // Right side of the Planner to the left side of the Builder: no loop under or over the cards.
   expect(g.pts[0]).toEqual({x:a.x+a.w,y:a.y+a.h/2});
   expect(g.pts[3]).toEqual({x:b.x,y:b.y+b.h/2});
   const ys=bezierPoints(g.pts).map(p=>p.y),xs=bezierPoints(g.pts).map(p=>p.x);
   expect(Math.min(...xs)).toBeGreaterThanOrEqual(a.x+a.w);expect(Math.max(...xs)).toBeLessThanOrEqual(b.x);
   expect(Math.min(...ys)).toBeGreaterThanOrEqual(Math.min(g.pts[0].y,g.pts[3].y));expect(Math.max(...ys)).toBeLessThanOrEqual(Math.max(g.pts[0].y,g.pts[3].y));
  }
 }
});
it('in Logos style the fan still runs port to port, between the logo columns',()=>{
 const logos=logoRouteBoxes(planner),routes=routeEdges(fan,logos);
 for(const e of fan){const g=routes.get(e)!,a=logos.planner,b=logos[e.to];
  expect(g.pts[0]).toEqual({x:a.x+a.w,y:a.y+a.h/2});expect(g.pts[3]).toEqual({x:b.x,y:b.y+b.h/2});}
});
it('still goes round a card that is genuinely in the way of the fan',()=>{
 const seats={...planner,mid:{x:270,y:250,w:20,h:60}};
 const g=routeEdges(fan,seats).get(fan[1])!;const c=seats.mid;
 expect(bezierPoints(g.pts).some(p=>p.x>c.x-10&&p.x<c.x+c.w+10&&p.y>c.y-10&&p.y<c.y+c.h+10)).toBe(false);
});
