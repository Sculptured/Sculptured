use crate::{
    CompiledEdge, CompiledPod, CompiledRig, CompiledSeat, Diagnostic, EdgeKind, MemberSpec,
    PodSpec, RestorePolicy, RigSpec, Runtime, Severity,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// Longest quoted value a diagnostic may echo from the spec file, in chars.
const MAX_QUOTED: usize = 40;

/// Caps every `"…"` or `` `…` `` span in a serde message at `MAX_QUOTED` chars, so a
/// diagnostic about a value never echoes much of the file it came from (the spec
/// path can point at any readable file, and 422 bodies are stored as receipts).
fn cap_quoted(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut chars = msg.chars();
    while let Some(c) = chars.next() {
        out.push(c);
        if c != '"' && c != '`' {
            continue;
        }
        let (inner, closed) = quoted_span(&mut chars, c);
        push_capped(&mut out, &inner);
        if closed {
            out.push(c);
        }
    }
    out
}

/// Reads a quoted span's body up to its closing `quote`: (body, whether it closed).
fn quoted_span(chars: &mut std::str::Chars<'_>, quote: char) -> (String, bool) {
    let mut inner = String::new();
    while let Some(d) = chars.next() {
        if d == '\\' && quote == '"' {
            inner.push(d);
            if let Some(e) = chars.next() {
                inner.push(e);
            }
            continue;
        }
        if d == quote {
            return (inner, true);
        }
        inner.push(d);
    }
    (inner, false)
}

fn push_capped(out: &mut String, inner: &str) {
    if inner.chars().count() > MAX_QUOTED {
        out.extend(inner.chars().take(MAX_QUOTED));
        out.push('…');
    } else {
        out.push_str(inner);
    }
}

fn yaml_error(e: serde_yaml_ng::Error) -> Vec<Diagnostic> {
    let path = e
        .location()
        .map(|l| format!("line {}:{}", l.line(), l.column()))
        .unwrap_or_else(|| "spec".into());
    vec![crate::diag::error(path, cap_quoted(&e.to_string()))]
}

pub fn parse(yaml: &str) -> Result<RigSpec, Vec<Diagnostic>> {
    // A file that is not a mapping (a .env file, a key, prose) gets a fixed message:
    // serde would quote the whole document back.
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).map_err(yaml_error)?;
    if !doc.is_mapping() {
        return Err(vec![crate::diag::error(
            "spec",
            "not a team spec (expected a YAML mapping)",
        )]);
    }
    serde_yaml_ng::from_str(yaml).map_err(yaml_error)
}

/// Kahn's algorithm, keeping declaration order inside each layer.
/// `before_after` pairs mean "first launches before second". Returns the
/// unplaced seats on a cycle.
fn launch_layers(
    order: &[String],
    before_after: &[(String, String)],
) -> Result<Vec<Vec<String>>, Vec<String>> {
    let mut indeg: HashMap<String, usize> = order.iter().map(|s| (s.clone(), 0)).collect();
    for (_, after) in before_after {
        *indeg.get_mut(after).expect("validated endpoint") += 1;
    }
    let mut placed: HashSet<String> = HashSet::new();
    let mut layers = Vec::new();
    while placed.len() < order.len() {
        let layer: Vec<String> = order
            .iter()
            .filter(|s| !placed.contains(*s) && indeg[*s] == 0)
            .cloned()
            .collect();
        if layer.is_empty() {
            return Err(order
                .iter()
                .filter(|s| !placed.contains(*s))
                .cloned()
                .collect());
        }
        for (before, after) in before_after {
            if layer.contains(before) {
                *indeg
                    .get_mut(after)
                    .expect("edge endpoints were validated before scheduling") -= 1;
            }
        }
        placed.extend(layer.iter().cloned());
        layers.push(layer);
    }
    Ok(layers)
}

/// The (before, after) pairs that close a loop, found depth-first from the seats nothing
/// launches before, in declaration order. Without them the rest is a forward order. A loop that
/// no starting seat leads into keeps all its pairs, so it stays a launch-order cycle.
fn loop_closing(order: &[String], deps: &[(String, String)]) -> HashSet<(String, String)> {
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for (before, after) in deps {
        next.entry(before.as_str())
            .or_default()
            .push(after.as_str());
    }
    let has_before: HashSet<&str> = deps.iter().map(|(_, after)| after.as_str()).collect();
    let mut state: HashMap<&str, bool> = HashMap::new(); // false: on the current path; true: done
    let mut closing = HashSet::new();
    let matching_order = order.iter().filter(|s| !has_before.contains(s.as_str()));
    for root in matching_order {
        visit(root, &next, &mut state, &mut closing);
    }
    closing
}

fn visit<'a>(
    s: &'a str,
    next: &HashMap<&'a str, Vec<&'a str>>,
    state: &mut HashMap<&'a str, bool>,
    closing: &mut HashSet<(String, String)>,
) {
    if state.contains_key(s) {
        return;
    }
    state.insert(s, false);
    let matching_next = next.get(s).map(Vec::as_slice).unwrap_or(&[]);
    for &t in matching_next {
        match state.get(t) {
            Some(false) => {
                closing.insert((s.to_string(), t.to_string()));
            }
            Some(true) => {}
            None => visit(t, next, state, closing),
        }
    }
    state.insert(s, true);
}

pub(crate) fn seat_id(spec: &RigSpec, pod: &str, member: &str) -> String {
    format!("{pod}.{member}@{}", spec.name)
}

fn compile_seat(spec: &RigSpec, pod: &PodSpec, m: &MemberSpec) -> CompiledSeat {
    CompiledSeat {
        id: seat_id(spec, &pod.id, &m.id),
        pod: pod.id.clone(),
        member: m.id.clone(),
        runtime: Runtime::parse(&m.runtime).expect("validated"),
        cwd: m.cwd.clone(),
        agent_ref: m.agent_ref.clone(),
        profile: m.profile.clone(),
        label: m.label.clone(),
        role: m.role.clone(),
        model: m.model.clone(),
        restore_policy: m
            .restore_policy
            .as_deref()
            .and_then(RestorePolicy::parse)
            .unwrap_or(RestorePolicy::ResumeIfPossible),
        permission_policy: m
            .permission_policy
            .clone()
            .or_else(|| spec.permission_policy.clone()),
        connection: m
            .connection
            .clone()
            .filter(|_| matches!(Runtime::parse(&m.runtime), Some(Runtime::ClaudeCode))),
    }
}

fn compile_edge(kind: &str, from: String, to: String) -> CompiledEdge {
    let kind = EdgeKind::parse(kind).expect("validated");
    CompiledEdge {
        kind,
        from,
        to,
        enforced: kind.orders_launch(),
    }
}

/// Pod-local edges first, pod by pod, then cross-pod edges.
fn compile_edges(spec: &RigSpec) -> Vec<CompiledEdge> {
    let mut edges = Vec::new();
    for pod in &spec.pods {
        for e in &pod.edges {
            edges.push(compile_edge(
                &e.kind,
                seat_id(spec, &pod.id, &e.from),
                seat_id(spec, &pod.id, &e.to),
            ));
        }
    }
    for e in &spec.edges {
        let (fp, fm) = e.from.split_once('.').expect("validated");
        let (tp, tm) = e.to.split_once('.').expect("validated");
        edges.push(compile_edge(
            &e.kind,
            seat_id(spec, fp, fm),
            seat_id(spec, tp, tm),
        ));
    }
    edges
}

/// (launches first, launches after) pairs from the edges that order launch.
fn launch_deps(edges: &[CompiledEdge]) -> Vec<(String, String)> {
    edges
        .iter()
        .filter_map(|e| match e.kind {
            EdgeKind::DelegatesTo => Some((e.from.clone(), e.to.clone())),
            EdgeKind::SpawnedBy => Some((e.to.clone(), e.from.clone())),
            _ => None,
        })
        .collect()
}

pub fn compile(yaml: &str) -> Result<CompiledRig, Vec<Diagnostic>> {
    let spec = parse(yaml)?;
    let mut diags = crate::validate(&spec);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(diags);
    }
    let pods = spec
        .pods
        .iter()
        .map(|p| CompiledPod {
            id: p.id.clone(),
            label: p.label.clone(),
        })
        .collect();
    let seats: Vec<CompiledSeat> = spec
        .pods
        .iter()
        .flat_map(|pod| pod.members.iter().map(|m| compile_seat(&spec, pod, m)))
        .collect();
    let edges = compile_edges(&spec);
    let order: Vec<String> = seats.iter().map(|s| s.id.clone()).collect();
    let deps = launch_deps(&edges);
    let closing = loop_closing(&order, &deps);
    let forward: Vec<(String, String)> =
        deps.into_iter().filter(|d| !closing.contains(d)).collect();
    let launch_layers = match launch_layers(&order, &forward) {
        Ok(l) => l,
        Err(cycle) => {
            diags.push(crate::diag::error(
                "edges",
                format!("launch-order cycle among: {}", cycle.join(", ")),
            ));
            return Err(diags);
        }
    };
    Ok(CompiledRig {
        name: spec.name.clone(),
        summary: spec.summary.clone(),
        spec_hash: format!("sha256:{}", hex::encode(Sha256::digest(yaml.as_bytes()))),
        pods,
        seats,
        edges,
        launch_layers,
        diagnostics: diags,
    })
}
