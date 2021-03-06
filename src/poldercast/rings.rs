use crate::{
    Address, GossipsBuilder, Layer, Node, NodeProfile, Nodes, Selection, Subscription,
    Subscriptions, Topic, ViewBuilder,
};
use std::collections::BTreeMap;

/// the number of neighbor for a given subscribed topic of the given node.
///
/// although the protocol only requires a view of length 2 (i.e. one
/// predecessor and one successor), we keep an additional predecessor
/// and successor in case of failures or node churn.
const RINGS_MAX_VIEW_SIZE: usize = 4;

/// see [`RINGS_MAX_VIEW_SIZE`]
const RINGS_NEIGHBOR_PREDECESSOR_SIZE: usize = RINGS_MAX_VIEW_SIZE / 2;
/// see [`RINGS_MAX_VIEW_SIZE`]
const RINGS_NEIGHBOR_SUCCESSOR_SIZE: usize = RINGS_MAX_VIEW_SIZE / 2;

/// this object is responsible for maintaining the ring links
/// of the node.
///
/// Ring module of the node, it manages the ring links.
/// It aims at discovering a node’s successor and predecessor
/// for each topic in its subscription, and at quickly adapting
/// to new successors/predecessors in dynamic networks.
///
#[derive(Clone, Debug)]
pub struct Rings {
    /// each node maintains `RINGS_MAX_VIEW_SIZE` neighbors for each topic
    /// in its subscription: `RINGS_NEIGHBOR_PREDECESSOR` with lower
    /// and `RINGS_NEIGHBOR_SUCCESSOR` with higher id.
    neighbors: BTreeMap<Topic, TopicView>,
}

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
enum Slot<A> {
    Taken(A),
    Available,
}

/// the Rings' Topic view, [`Id`] of the nodes that are subscribed
/// to a given Topic.
///
/// This structure is mainly necessary for the [`Rings`] module.
///
/// [`Rings`]: ./node/ring/struct.Rings.html
#[derive(Clone, Debug)]
struct TopicView([Slot<Address>; RINGS_MAX_VIEW_SIZE]);

impl Layer for Rings {
    fn alias(&self) -> &'static str {
        "rings"
    }

    fn reset(&mut self) {
        self.neighbors = BTreeMap::new()
    }

    fn populate(&mut self, identity: &NodeProfile, all_nodes: &Nodes) {
        self.update_view(identity, all_nodes)
    }

    fn gossips(&mut self, identity: &NodeProfile, gossips: &mut GossipsBuilder, all_nodes: &Nodes) {
        self.select_nodes_to_send(identity, gossips, all_nodes)
    }

    fn view(&mut self, view: &mut ViewBuilder, all_nodes: &mut Nodes) {
        match *view.selection() {
            Selection::Any => {
                for neighborhood in self.neighbors.values() {
                    neighborhood.populate_view(view.origin().cloned(), view, all_nodes);
                }
            }
            Selection::Topic { topic } => {
                if let Some(neighborhood) = self.neighbors.get(&topic) {
                    neighborhood.populate_view(view.origin().cloned(), view, all_nodes);
                }
            }
        }
    }
}

impl<A> Slot<A> {
    fn option(&self) -> Option<&A> {
        match self {
            Slot::Taken(a) => Some(a),
            Slot::Available => None,
        }
    }
}

impl Default for TopicView {
    fn default() -> Self {
        TopicView([
            Slot::Available,
            Slot::Available,
            Slot::Available,
            Slot::Available,
        ])
    }
}

impl Default for Rings {
    fn default() -> Self {
        Rings {
            neighbors: BTreeMap::default(),
        }
    }
}

impl TopicView {
    fn populate_view<'a>(
        &self,
        from: Option<Address>,
        view_builder: &mut ViewBuilder,
        all_nodes: &'a mut Nodes,
    ) {
        if let Some(from) = from {
            if self.is_predecessor(&from) {
                for node in self.successors().filter_map(|slot| slot.option()) {
                    if let Some(node) = all_nodes.peek_mut(node) {
                        view_builder.add(node)
                    }
                }
            } else if self.is_successor(&from) {
                for node in self.predecessors().filter_map(|slot| slot.option()) {
                    if let Some(node) = all_nodes.peek_mut(node) {
                        view_builder.add(node)
                    }
                }
            } else {
                for node in self.iter().filter_map(|slot| slot.option()) {
                    if let Some(node) = all_nodes.peek_mut(node) {
                        view_builder.add(node)
                    }
                }
            }
        } else {
            for node in self.iter().filter_map(|slot| slot.option()) {
                if let Some(node) = all_nodes.peek_mut(node) {
                    view_builder.add(node)
                }
            }
        }
    }

    fn is_predecessor(&self, public_id: &Address) -> bool {
        self.predecessors()
            .filter_map(|slot| slot.option())
            .any(|node| node == public_id)
    }

    fn is_successor(&self, public_id: &Address) -> bool {
        self.successors()
            .filter_map(|slot| slot.option())
            .any(|node| node == public_id)
    }

    /// return an iterator over the successors in this [`TopicView`].
    ///
    /// In the case [`RINGS_NEIGHBOR_SUCCESSOR_SIZE`] allows more than
    /// one successor, the successors in this iterator are sorted from
    /// the closest to the node to the farthest (i.e. they are in the
    /// order of interest already.)
    ///
    /// This function returns the [`Slot`] as well, this is so we can
    /// modify the slot in the rings module when we need to add new items
    /// in the slot
    fn successors(&self) -> impl Iterator<Item = &Slot<Address>> {
        self.0
            .iter()
            .skip(RINGS_NEIGHBOR_PREDECESSOR_SIZE)
            .take(RINGS_NEIGHBOR_SUCCESSOR_SIZE)
    }

    /// return a mutable iterator over the successors in this [`TopicView`].
    ///
    /// In the case [`RINGS_NEIGHBOR_SUCCESSOR_SIZE`] allows more than
    /// one successor, the successors in this iterator are sorted from
    /// the closest to the node to the farthest (i.e. they are in the
    /// order of interest already.)
    fn successors_mut(&mut self) -> impl Iterator<Item = &mut Slot<Address>> {
        self.0
            .iter_mut()
            .skip(RINGS_NEIGHBOR_PREDECESSOR_SIZE)
            .take(RINGS_NEIGHBOR_SUCCESSOR_SIZE)
    }

    /// return an iterator over the predecessors in this [`TopicView`].
    ///
    /// In the case [`RINGS_NEIGHBOR_PREDECESSOR_SIZE`] allows more than
    /// one predecessor, the predecessors in this iterator are sorted from
    /// the closest to the node to the farthest (i.e. they are in the
    /// order of interest already.)
    fn predecessors(&self) -> impl Iterator<Item = &Slot<Address>> {
        self.0
            .iter()
            .rev()
            .skip(RINGS_NEIGHBOR_SUCCESSOR_SIZE)
            .take(RINGS_NEIGHBOR_PREDECESSOR_SIZE)
    }

    /// return a mutable iterator over the predecessors in this [`TopicView`].
    ///
    /// In the case [`RINGS_NEIGHBOR_PREDECESSOR_SIZE`] allows more than
    /// one predecessor, the predecessors in this iterator are sorted from
    /// the closest to the node to the farthest (i.e. they are in the
    /// order of interest already.)
    fn predecessors_mut(&mut self) -> impl Iterator<Item = &mut Slot<Address>> {
        self.0
            .iter_mut()
            .rev()
            .skip(RINGS_NEIGHBOR_SUCCESSOR_SIZE)
            .take(RINGS_NEIGHBOR_PREDECESSOR_SIZE)
    }

    /// iterator over every neighbors, not ordered by preferences (see
    /// [`predecessors`] and [`successors`] for preference ordered iterators)
    fn iter(&self) -> impl Iterator<Item = &Slot<Address>> {
        self.0.iter()
    }
}

impl Rings {
    // update the Rings view (neighbors for every topics) with the given new nodes
    fn update_view(&mut self, self_node: &NodeProfile, all_nodes: &Nodes) {
        if let Some(self_address) = self_node.address() {
            self.neighbors = BTreeMap::new();

            let known_nodes = all_nodes.available_nodes();
            let known_nodes = known_nodes
                .iter()
                .filter_map(|id| all_nodes.peek(id).map(|v| (id, v)))
                .filter(|(_, node)| node.profile().address().is_some())
                .collect();

            for subscription in self_node.subscriptions().iter() {
                let view = select_best_nodes_for_topic(self_address, *subscription, &known_nodes);

                self.neighbors.insert(subscription.topic(), view);
            }
        }
    }

    fn select_nodes_to_send(
        &self,
        self_node: &NodeProfile,
        gossip_builder: &mut GossipsBuilder,
        all_nodes: &Nodes,
    ) {
        let gossip_node_id = gossip_builder.recipient().clone();
        if let Some(gossip_node) = all_nodes.peek(&gossip_node_id) {
            // these are the subscriptions in common between the gossip node and our nodes
            let common_topics: Subscriptions = self_node
                .common_subscriptions(&gossip_node.profile())
                .cloned()
                .collect();

            // candidates are the one that are common topics.
            let candidates: BTreeMap<&Address, &Node> = all_nodes
                .available_nodes()
                .iter()
                .filter_map(|id| all_nodes.peek(id))
                .filter(|node| node.profile().address().is_some())
                .filter(|v| {
                    v.profile()
                        .subscriptions()
                        .common_subscriptions(&common_topics)
                        .next()
                        .is_some()
                })
                .map(|v| (v.address(), v))
                .collect();

            for topic in common_topics.iter() {
                let view = select_best_nodes_for_topic(&gossip_node_id, *topic, &candidates);

                for candidate in view.iter().filter_map(|v| v.option()) {
                    gossip_builder.add(candidate.clone());
                }
            }
        }
    }
}

fn select_best_nodes_for_topic(
    other_id: &Address,
    subscription: Subscription,
    candidates: &BTreeMap<&Address, &Node>,
) -> TopicView {
    use std::ops::Bound::{Excluded, Unbounded};
    let mut view = TopicView::default();

    {
        // these are the predecessor
        let mut predecessor = view.predecessors_mut();
        for (candidate_id, candidate) in candidates
            .range::<&Address, _>((Unbounded, Excluded(other_id)))
            .rev()
        {
            if candidate
                .profile()
                .subscriptions()
                .contains(subscription.topic())
            {
                if let Some(p) = predecessor.next() {
                    *p = Slot::Taken(Address::clone(candidate_id));
                } else {
                    // we can stop as soon as we have all the necessary element
                    break;
                }
            }
        }
    }

    {
        // these are the successor of the topic
        let mut successor = view.successors_mut();
        for (candidate_id, candidate) in candidates
            .range::<&Address, _>((Excluded(other_id), Unbounded))
            .rev()
        {
            if candidate
                .profile()
                .subscriptions()
                .contains(subscription.topic())
            {
                if let Some(p) = successor.next() {
                    *p = Slot::Taken(Address::clone(candidate_id));
                } else {
                    // we can stop as soon as we have all the necessary element
                    break;
                }
            }
        }
    }

    view
}
