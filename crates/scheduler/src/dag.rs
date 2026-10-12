//! Grafo de dependencias de tareas (P08.S5). Las aristas son `(tarea, de_la_que_depende)`.
//! Funciones puras; el almacén aplica la misma regla de ciclos con SQL.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Los nodos que quedaron sin poder ordenarse: forman (o cuelgan de) un ciclo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle(pub Vec<String>);

/// ¿Añadir `task → depends_on` cerraría un ciclo? (`depends_on` ya depende, directa o
/// indirectamente, de `task`, o es la misma tarea.)
pub fn creates_cycle(edges: &[(String, String)], task: &str, depends_on: &str) -> bool {
    if task == depends_on {
        return true;
    }
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in edges {
        deps.entry(a.as_str()).or_default().push(b.as_str());
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack = vec![depends_on];
    while let Some(n) = stack.pop() {
        if n == task {
            return true;
        }
        if seen.insert(n) {
            stack.extend(deps.get(n).into_iter().flatten().copied());
        }
    }
    false
}

/// Olas por profundidad (Kahn): la ola `k` solo depende de olas anteriores. Dentro de cada
/// ola el orden es alfabético, para que sea determinista.
pub fn topo_layers(
    nodes: impl IntoIterator<Item = String>,
    edges: &[(String, String)],
) -> Result<Vec<Vec<String>>, Cycle> {
    let mut pending: BTreeMap<String, BTreeSet<String>> =
        nodes.into_iter().map(|n| (n, BTreeSet::new())).collect();
    for (a, b) in edges {
        // Las aristas hacia nodos que no existen se ignoran: las valida quien arma el grafo.
        if pending.contains_key(b) {
            pending.entry(a.clone()).or_default().insert(b.clone());
        }
    }
    let mut layers = Vec::new();
    while !pending.is_empty() {
        let ready: Vec<String> = pending
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(n, _)| n.clone())
            .collect();
        if ready.is_empty() {
            return Err(Cycle(pending.into_keys().collect()));
        }
        for n in &ready {
            pending.remove(n);
        }
        for deps in pending.values_mut() {
            for n in &ready {
                deps.remove(n);
            }
        }
        layers.push(ready);
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect()
    }

    #[test]
    fn a_closing_edge_is_a_cycle_a_new_branch_is_not() {
        let edges = e(&[("b", "a"), ("c", "b")]);
        assert!(creates_cycle(&edges, "a", "c"), "a→c cierra a←b←c");
        assert!(creates_cycle(&edges, "a", "b"));
        assert!(creates_cycle(&edges, "x", "x"));
        assert!(!creates_cycle(&edges, "c", "a"), "atajo, no ciclo");
        assert!(!creates_cycle(&edges, "d", "c"));
    }

    #[test]
    fn layers_follow_depth_and_are_deterministic() {
        let nodes = ["d", "c", "b", "a", "z"].map(String::from);
        let layers =
            topo_layers(nodes, &e(&[("b", "a"), ("c", "a"), ("d", "b"), ("d", "c")])).unwrap();
        assert_eq!(
            layers,
            vec![
                vec!["a".to_string(), "z".into()],
                vec!["b".into(), "c".into()],
                vec!["d".into()],
            ]
        );
    }

    #[test]
    fn a_cycle_is_reported_with_its_members() {
        let nodes = ["a", "b", "c"].map(String::from);
        let err = topo_layers(nodes, &e(&[("a", "b"), ("b", "a")])).unwrap_err();
        assert_eq!(err, Cycle(vec!["a".into(), "b".into()]));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Un DAG nunca acepta ciclos (PLAN P08.S5): se arma un grafo solo con aristas
            /// que `creates_cycle` deja pasar y siempre se puede ordenar en olas; y toda
            /// arista que él rechaza cerraría de verdad un ciclo.
            #[test]
            fn a_dag_built_with_creates_cycle_never_has_a_cycle(
                pairs in proptest::collection::vec((0u8..8, 0u8..8), 0..40)
            ) {
                let mut edges: Vec<(String, String)> = Vec::new();
                for (a, b) in pairs {
                    let (a, b) = (format!("n{a}"), format!("n{b}"));
                    if creates_cycle(&edges, &a, &b) {
                        // Rechazada: con la arista puesta habría un ciclo.
                        let mut with = edges.clone();
                        with.push((a, b));
                        let nodes: Vec<String> = (0..8).map(|i| format!("n{i}")).collect();
                        prop_assert!(topo_layers(nodes, &with).is_err());
                    } else {
                        edges.push((a, b));
                    }
                }
                let nodes: Vec<String> = (0..8).map(|i| format!("n{i}")).collect();
                let layers = topo_layers(nodes, &edges);
                prop_assert!(layers.is_ok(), "el grafo aceptado tiene un ciclo");
            }
        }
    }
}
