/** A small bar sparkline: one bar per value, scaled to the largest. Decorative; the number beside it is the reading. */
export function Spark({ values, label }: { values: number[]; label: string }) {
  const top = Math.max(1, ...values);
  return (
    <span className="ctl-spark" role="img" aria-label={label}>
      {values.map((v, i) => <i key={i} style={{ height: `${Math.max(v > 0 ? 8 : 2, (v / top) * 100)}%` }} />)}
    </span>
  );
}
