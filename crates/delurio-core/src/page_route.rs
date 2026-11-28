//! A page's route, held to the navigation graphs (SPEC.md §4, §6): a page node of ui.yaml may say the path it opens at,
//! `route`, written as a navigation graph writes a page's route. Rehber links to the page by it. Where the folders
//! `check` is given hold one or more navigation graphs, a route that no page of any of them has is a warning,
//! `page-route-unknown`, which fails nothing; two routes are the same when only the names of their parameters differ,
//! `{name}` and `:name` read alike. With no navigation graph, no route is held. That `route` is only on a page that is
//! not a dialog is the schema's.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::graph::{Graph, is_dialog};

pub const PAGE_ROUTE_UNKNOWN: &str = "page-route-unknown";

/// the routes of a navigation graph's pages: the graph's id, and each page's route as it is written
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRoutes {
    pub id: String,
    pub routes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteProblem {
    pub path: Vec<String>, // where in ui.yaml
    pub element: String,
    pub message: String,
}

/// a parameter written in braces: `{name}`, or `{...name}` for any number of segments
static PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\.\.\.)?[^{}/]*\}").expect("the pattern compiles"));

/// a route as SPEC.md §6 reads it, its parameters' names left out: `{name}` and a segment `:name` are `{}`, and
/// `{...name}` is `{...}`
pub fn route_shape(route: &str) -> String {
    route
        .split('/')
        .map(|segment| {
            if segment.starts_with(':') {
                "{}".to_string()
            } else {
                let shape = |c: &Captures| if c.get(1).is_some() { "{...}" } else { "{}" };
                PARAM.replace_all(segment, shape).into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// each page node of a flow's screens that says its route, but a dialog's: the node and the route, in the order of
/// `nodes`; a route that is not a path (a string that starts with `/`) is left to the `schema` rule
pub fn page_routes(g: &Graph) -> Vec<(String, String)> {
    g.ui()
        .nodes()
        .filter(|(_, n)| n.kind() == Some("page") && !is_dialog(Some(*n)))
        .filter_map(|(id, n)| {
            let route = n.value().get_str("route").filter(|r| r.starts_with('/'))?;
            Some((id.to_string(), route.to_string()))
        })
        .collect()
}

/// the pages of a flow (node, route) whose route no page of `navs` has, each at the node's `route`; with no
/// navigation graph, none
pub fn unknown_routes(pages: &[(String, String)], navs: &[NavRoutes]) -> Vec<RouteProblem> {
    if navs.is_empty() {
        return Vec::new();
    }
    let known: Vec<String> = navs.iter().flat_map(|n| n.routes.iter().map(|r| route_shape(r))).collect();
    let ids: Vec<&str> = navs.iter().map(|n| n.id.as_str()).collect();
    let graphs = match ids.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("the navigation graphs {} and {last}", rest.join(", ")),
        _ => format!("the navigation graph {}", ids.join("")),
    };
    pages
        .iter()
        .filter(|(_, route)| !known.contains(&route_shape(route)))
        .map(|(node, route)| RouteProblem {
            path: vec!["nodes".to_string(), node.clone(), "route".to_string()],
            element: format!("nodes/{node}/route"),
            message: format!(
                "page {node} opens at {route}, and no page of {graphs} has that route, so Rehber cannot take a person to it (SPEC.md §4, §6)"
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(n, r)| (n.to_string(), r.to_string())).collect()
    }

    fn nav(id: &str, routes: &[&str]) -> NavRoutes {
        NavRoutes { id: id.to_string(), routes: routes.iter().map(|r| r.to_string()).collect() }
    }

    #[test]
    fn a_route_is_read_without_its_parameters_names() {
        assert_eq!(route_shape("/w/:workspace/plan"), "/w/{}/plan");
        assert_eq!(route_shape("/w/{ws}/plan"), "/w/{}/plan");
        assert_eq!(route_shape("/docs/{...slug}"), "/docs/{...}");
        assert_eq!(route_shape("/{...path}"), "/{...}");
        assert_eq!(route_shape("/ders/{id}/celseler"), "/ders/{}/celseler");
        assert_eq!(route_shape("/"), "/");
        // a catch-all is not one segment, and a trailing slash is a route of its own
        assert_ne!(route_shape("/docs/{slug}"), route_shape("/docs/{...slug}"));
        assert_ne!(route_shape("/w/:workspace/plan/"), route_shape("/w/:workspace/plan"));
    }

    #[test]
    fn a_page_s_route_is_held_to_the_navigation_graphs_pages() {
        let panel = nav("demo.panel", &["/", "/w/{workspace}/plan", "/w/{workspace}/members"]);
        let site = nav("demo.site", &["/", "/pricing"]);

        // a route a navigation page has, its parameter named otherwise: nothing
        let plan = pages(&[("planPage", "/w/:ws/plan")]);
        assert_eq!(unknown_routes(&plan, std::slice::from_ref(&panel)), []);

        // a route no navigation page has: a warning at the node's route, which names it
        let billing = pages(&[("planPage", "/w/:ws/billing"), ("members", "/w/{id}/members")]);
        let found = unknown_routes(&billing, std::slice::from_ref(&panel));
        assert_eq!(
            found,
            [RouteProblem {
                path: vec!["nodes".to_string(), "planPage".to_string(), "route".to_string()],
                element: "nodes/planPage/route".to_string(),
                message: "page planPage opens at /w/:ws/billing, and no page of the navigation graph demo.panel has that route, so Rehber cannot take a person to it (SPEC.md §4, §6)".to_string(),
            }]
        );

        // every navigation graph's pages count, and the message names them all
        assert_eq!(unknown_routes(&pages(&[("pricing", "/pricing")]), &[panel.clone(), site.clone()]), []);
        let found = unknown_routes(&pages(&[("about", "/about")]), &[panel, site]);
        assert_eq!(
            found[0].message,
            "page about opens at /about, and no page of the navigation graphs demo.panel and demo.site has that route, so Rehber cannot take a person to it (SPEC.md §4, §6)"
        );

        // no navigation graph: no route is held
        assert_eq!(unknown_routes(&pages(&[("planPage", "/w/:ws/billing")]), &[]), []);
    }

    #[test]
    fn only_a_page_that_is_no_dialog_says_its_route() {
        let mut g = crate::graph::load(&crate::changes::tests::example("notes/edit")).expect("the example loads");
        assert_eq!(page_routes(&g), []);
        let Some(crate::js::Value::Object(nodes)) = g.ui.get_mut("nodes") else { panic!("nodes") };
        for (node, route) in [("page", "/notes/{id}/edit"), ("discardDialog", "/notes/{id}/discard"), ("title", "/x")] {
            let Some(crate::js::Value::Object(n)) = nodes.get_mut(node) else { panic!("node {node}") };
            n.insert("route", crate::js::Value::from(route));
        }
        // a route that is not a path is the schema's to refuse, and not held to the navigation graphs as well
        let other = crate::js::json_parse(r#"{"type":"page","role":"main","route":"notes/{id}"}"#).expect("JSON");
        nodes.insert("other", other);
        assert_eq!(page_routes(&g), pages(&[("page", "/notes/{id}/edit")]));
    }
}
