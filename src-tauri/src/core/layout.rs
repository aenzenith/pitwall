//! How the terminal panel lays a project's terminals out beside and under each other (its
//! Düzen, `window/TerminalPanel.vue`): a tree of splits whose leaves are terminals or empty
//! panes. Held while Pitwall runs as the UI last gave it. The quit's record (`resume.rs`) and an
//! update restart's (`restore.rs`) write it down by the tabs' places in that record, never a
//! terminal's id nor anything a terminal showed, and it comes back with the tabs that do.

use serde::{Deserialize, Serialize};

use super::*;

/// More than the panel ever makes: a layout past these is no layout of its.
const MAX_PANES: usize = 16;
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    /// Side by side.
    Row,
    /// One under the other.
    Col,
}

/// A pane (a terminal, or empty), or panes split side by side or one under the other, each kid
/// taking its share (`sizes`) of the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PaneNode {
    Pane { terminal: Option<u64> },
    Split { dir: SplitDir, sizes: Vec<f64>, kids: Vec<PaneNode> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalLayout {
    pub tree: PaneNode,
    /// The terminal whose pane has the keyboard.
    #[serde(default)]
    pub focus: Option<u64>,
    /// The terminal whose pane fills the panel for now (Büyüt).
    #[serde(default)]
    pub zoom: Option<u64>,
    /// Moves when the core lays it itself (put back after a quit or an update): the UI then
    /// takes it over the one it has.
    #[serde(default)]
    pub rev: u64,
}

/// A layout as a record keeps it: a pane names its tab by the tab's place in the record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum SavedNode {
    Pane { tab: Option<usize> },
    Split { dir: SplitDir, sizes: Vec<f64>, kids: Vec<SavedNode> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SavedLayout {
    tree: SavedNode,
    #[serde(default)]
    focus: Option<usize>,
    #[serde(default)]
    zoom: Option<usize>,
}

/// Either tree, by what its panes hold.
enum Tree<L> {
    Pane(L),
    Split(SplitDir, Vec<f64>, Vec<Tree<L>>),
}

impl From<PaneNode> for Tree<Option<u64>> {
    fn from(node: PaneNode) -> Self {
        match node {
            PaneNode::Pane { terminal } => Tree::Pane(terminal),
            PaneNode::Split { dir, sizes, kids } => Tree::Split(dir, sizes, kids.into_iter().map(Tree::from).collect()),
        }
    }
}

impl From<Tree<Option<u64>>> for PaneNode {
    fn from(tree: Tree<Option<u64>>) -> Self {
        match tree {
            Tree::Pane(terminal) => PaneNode::Pane { terminal },
            Tree::Split(dir, sizes, kids) => PaneNode::Split { dir, sizes, kids: kids.into_iter().map(PaneNode::from).collect() },
        }
    }
}

impl From<SavedNode> for Tree<Option<usize>> {
    fn from(node: SavedNode) -> Self {
        match node {
            SavedNode::Pane { tab } => Tree::Pane(tab),
            SavedNode::Split { dir, sizes, kids } => Tree::Split(dir, sizes, kids.into_iter().map(Tree::from).collect()),
        }
    }
}

impl From<Tree<Option<usize>>> for SavedNode {
    fn from(tree: Tree<Option<usize>>) -> Self {
        match tree {
            Tree::Pane(tab) => SavedNode::Pane { tab },
            Tree::Split(dir, sizes, kids) => SavedNode::Split { dir, sizes, kids: kids.into_iter().map(SavedNode::from).collect() },
        }
    }
}

/// What becomes of a pane: kept, holding this, or gone (its tab didn't come along), the room
/// going to its neighbours.
enum Becomes<M> {
    Kept(M),
    Gone,
}

/// The tree with each pane mapped. `Err`: a tree the panel never makes (too deep, a split of
/// fewer than two, shares that aren't positive or don't match the kids); `Ok(None)`: no pane
/// left. A split left with one kid gives way to it, and the kept kids' shares fill the room.
fn mapped<L, M>(tree: Tree<L>, depth: usize, pane: &mut impl FnMut(L) -> Becomes<M>) -> Result<Option<Tree<M>>, ()> {
    match tree {
        Tree::Pane(leaf) => Ok(match pane(leaf) {
            Becomes::Kept(kept) => Some(Tree::Pane(kept)),
            Becomes::Gone => None,
        }),
        Tree::Split(dir, sizes, kids) => {
            if depth >= MAX_DEPTH || kids.len() < 2 || kids.len() > MAX_PANES || sizes.len() != kids.len() || sizes.iter().any(|share| !share.is_finite() || *share <= 0.0) {
                return Err(());
            }
            let mut kept: Vec<(f64, Tree<M>)> = Vec::new();
            for (share, kid) in sizes.into_iter().zip(kids) {
                if let Some(kid) = mapped(kid, depth + 1, pane)? {
                    kept.push((share, kid));
                }
            }
            Ok(match kept.len() {
                0 => None,
                1 => kept.pop().map(|(_, kid)| kid),
                _ => {
                    let total: f64 = kept.iter().map(|(share, _)| share).sum();
                    let (sizes, kids) = kept.into_iter().map(|(share, kid)| (share / total, kid)).unzip();
                    Some(Tree::Split(dir, sizes, kids))
                }
            })
        }
    }
}

fn panes<L>(tree: &Tree<L>) -> Vec<&L> {
    match tree {
        Tree::Pane(leaf) => vec![leaf],
        Tree::Split(_, _, kids) => kids.iter().flat_map(panes).collect(),
    }
}

/// No more panes than the panel makes, no terminal in two of them.
fn sound<T: PartialEq>(tree: &Tree<Option<T>>) -> bool {
    let all = panes(tree);
    let held: Vec<&T> = all.iter().filter_map(|leaf| leaf.as_ref()).collect();
    all.len() <= MAX_PANES && held.iter().enumerate().all(|(at, one)| !held[..at].contains(one))
}

impl TerminalLayout {
    /// One the UI gave, if the panel could have made it.
    pub(super) fn checked(self) -> Option<Self> {
        let tree = mapped(Tree::from(self.tree), 0, &mut Becomes::Kept).ok()??;
        sound(&tree).then(|| {
            let held = |id: Option<u64>| id.filter(|id| panes(&tree).contains(&&Some(*id)));
            let (focus, zoom) = (held(self.focus), held(self.zoom));
            TerminalLayout { tree: PaneNode::from(tree), focus, zoom, rev: self.rev }
        })
    }

    /// Written down by the tabs' places in a record (`place`: a terminal's place among its
    /// tabs). A terminal that has none leaves its pane; with fewer than two panes left there is
    /// no layout to keep.
    pub(super) fn saved(&self, place: impl Fn(u64) -> Option<usize>) -> Option<SavedLayout> {
        let tree = mapped(Tree::from(self.tree.clone()), 0, &mut |pane: Option<u64>| match pane {
            None => Becomes::Kept(None),
            Some(id) => place(id).map_or(Becomes::Gone, |at| Becomes::Kept(Some(at))),
        })
        .ok()??;
        (panes(&tree).len() >= 2 && sound(&tree)).then(|| SavedLayout { tree: SavedNode::from(tree), focus: self.focus.and_then(&place), zoom: self.zoom.and_then(&place) })
    }
}

impl SavedLayout {
    /// Put back with the tabs that came back (`terminal`: the terminal a place came back as;
    /// none for one that stayed away, whose pane goes). Fewer than two panes: no layout.
    pub(super) fn restored(self, terminal: impl Fn(usize) -> Option<u64>, rev: u64) -> Option<TerminalLayout> {
        let tree = mapped(Tree::from(self.tree), 0, &mut |pane: Option<usize>| match pane {
            None => Becomes::Kept(None),
            Some(at) => terminal(at).map_or(Becomes::Gone, |id| Becomes::Kept(Some(id))),
        })
        .ok()??;
        if panes(&tree).len() < 2 || !sound(&tree) {
            return None;
        }
        TerminalLayout { tree: PaneNode::from(tree), focus: self.focus.and_then(&terminal), zoom: self.zoom.and_then(&terminal), rev }.checked()
    }
}

impl Core {
    /// The panel's layout of a project's terminals, as the UI has it now; none, or one the panel
    /// couldn't have made: one terminal at a time.
    pub fn set_terminal_layout(&self, path: &str, layout: Option<TerminalLayout>) {
        let mut inner = self.lock();
        match layout.and_then(TerminalLayout::checked) {
            Some(layout) => {
                inner.terminal_layouts.insert(path.to_string(), layout);
            }
            None => {
                inner.terminal_layouts.remove(path);
            }
        }
    }

    /// The terminal panel's height for its panes, as the UI last had it: kept for the next start.
    pub fn set_panes_height(&self, height: u32) {
        let height = height.clamp(120, 4000);
        {
            let mut inner = self.lock();
            if inner.settings.panes_height == Some(height) {
                return;
            }
            inner.settings.panes_height = Some(height);
        }
        self.save_settings();
    }

    /// A layout put back after a quit or an update, under a `rev` the UI hasn't seen.
    pub(super) fn lay_terminals(&self, path: &str, saved: SavedLayout, terminal: impl Fn(usize) -> Option<u64>) -> bool {
        let Some(layout) = saved.restored(terminal, now_ms()) else {
            return false;
        };
        self.lock().terminal_layouts.insert(path.to_string(), layout);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(terminal: Option<u64>) -> PaneNode {
        PaneNode::Pane { terminal }
    }

    /// Written down at a quit and put back at the next start, a layout keeps its panes' places
    /// and shares; a tab that doesn't come back (its session runs elsewhere by then) leaves its
    /// pane to its neighbour, and an empty pane the user made stays.
    #[test]
    fn a_layout_comes_back_with_the_tabs_that_do() {
        let layout = TerminalLayout {
            tree: PaneNode::Split {
                dir: SplitDir::Col,
                sizes: vec![0.75, 0.25],
                kids: vec![
                    PaneNode::Split { dir: SplitDir::Row, sizes: vec![0.625, 0.375], kids: vec![pane(Some(11)), pane(Some(12))] },
                    PaneNode::Split { dir: SplitDir::Row, sizes: vec![0.5, 0.5], kids: vec![pane(Some(13)), pane(None)] },
                ],
            },
            focus: Some(12),
            zoom: None,
            rev: 0,
        };
        let places = [11, 12, 13];
        let saved = layout.saved(|id| places.iter().position(|place| *place == id)).expect("written down");
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("11") && !json.contains("13"), "a record names tabs by place, never by a terminal's id: {json}");

        // The second tab stays away; the others come back as 21 and 23.
        let back: SavedLayout = serde_json::from_str(&json).unwrap();
        let restored = back.restored(|at| [Some(21), None, Some(23)][at], 7).expect("put back");
        assert_eq!(
            restored,
            TerminalLayout {
                tree: PaneNode::Split {
                    dir: SplitDir::Col,
                    sizes: vec![0.75, 0.25],
                    kids: vec![pane(Some(21)), PaneNode::Split { dir: SplitDir::Row, sizes: vec![0.5, 0.5], kids: vec![pane(Some(23)), pane(None)] }],
                },
                focus: None,
                zoom: None,
                rev: 7,
            }
        );

        // No tab back: the empty pane alone is no layout.
        assert_eq!(serde_json::from_str::<SavedLayout>(&json).unwrap().restored(|_| None, 7), None);
    }

    /// The record is a file anyone running as the user can write: what isn't a layout the panel
    /// makes puts back none, and never takes a terminal into two panes.
    #[test]
    fn a_record_that_isnt_a_layout_puts_back_none() {
        let back = |json: &str| serde_json::from_str::<SavedLayout>(json).ok().and_then(|saved| saved.restored(|at| Some(at as u64 + 1), 1));
        let leaf = r#"{"kind":"pane","tab":0}"#;
        assert!(back(&format!(r#"{{"tree":{{"kind":"split","dir":"row","sizes":[0.5,0.5],"kids":[{leaf},{{"kind":"pane","tab":1}}]}}}}"#)).is_some());
        // Shares that don't match, aren't positive, a split of one, a terminal twice, too deep.
        assert_eq!(back(&format!(r#"{{"tree":{{"kind":"split","dir":"row","sizes":[1],"kids":[{leaf},{leaf}]}}}}"#)), None);
        assert_eq!(back(&format!(r#"{{"tree":{{"kind":"split","dir":"row","sizes":[1,-1],"kids":[{leaf},{{"kind":"pane","tab":1}}]}}}}"#)), None);
        assert_eq!(back(&format!(r#"{{"tree":{{"kind":"split","dir":"col","sizes":[1],"kids":[{leaf}]}}}}"#)), None);
        assert_eq!(back(&format!(r#"{{"tree":{{"kind":"split","dir":"row","sizes":[0.5,0.5],"kids":[{leaf},{leaf}]}}}}"#)), None);
        let mut deep = format!(r#"{{"kind":"split","dir":"row","sizes":[1,1],"kids":[{leaf},{{"kind":"pane","tab":1}}]}}"#);
        for at in 2..12 {
            deep = format!(r#"{{"kind":"split","dir":"col","sizes":[1,1],"kids":[{deep},{{"kind":"pane","tab":{at}}}]}}"#);
        }
        assert_eq!(back(&format!(r#"{{"tree":{deep}}}"#)), None);
    }
}
