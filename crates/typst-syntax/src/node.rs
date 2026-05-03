use std::fmt::{self, Debug, Display, Formatter};
use std::ops::{Deref, Range};
use std::rc::Rc;
use std::sync::Arc;

use ecow::{EcoString, EcoVec, eco_format, eco_vec};

use crate::kind::ModeAfter;
use crate::{FileId, RangeMapper, Span, SyntaxKind, SyntaxMode};

/// A node in the untyped syntax tree.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct SyntaxNode(NodeTop);

/// A top level node containing a [`Span`] and [`SyntaxKind`] directly for
/// efficient access.
#[derive(Clone, Eq, PartialEq, Hash)]
enum NodeTop {
    Leaf(EcoString, Span, SyntaxKind),
    Inner(Arc<InnerNode>, Span, SyntaxKind),
    Error(Arc<ErrorNode>, Span, SyntaxKind),
    Warning(Arc<WarningWrapper>, Span, SyntaxKind),
}

/// A transparent wrapper around an actual node which allows us to add data,
/// currently just warning messages, without increasing the size of [`NodeTop`].
#[derive(Clone, Eq, PartialEq, Hash)]
enum NodeWrapper {
    Leaf(EcoString),
    Inner(Arc<InnerNode>),
    Error(Arc<ErrorNode>),
    /// A warning message wrapped directly around another node.
    Warning(Arc<WarningWrapper>),
}

/// A borrowed version of [`NodeWrapper`] for iterating past borrowed warnings.
#[derive(Clone, Copy)]
enum NodeRef<'a> {
    Node(Node<'a>),
    Warning(&'a WarningWrapper),
}

/// Data attached to a node.
#[derive(Clone, Copy)]
enum Node<'a> {
    Leaf(&'a EcoString),
    Inner(&'a Arc<InnerNode>),
    Error(&'a Arc<ErrorNode>),
}

impl NodeTop {
    fn as_ref(&self) -> NodeRef<'_> {
        match self {
            Self::Leaf(text, ..) => NodeRef::Node(Node::Leaf(text)),
            Self::Inner(inner, ..) => NodeRef::Node(Node::Inner(inner)),
            Self::Error(err, ..) => NodeRef::Node(Node::Error(err)),
            Self::Warning(warn, ..) => NodeRef::Warning(warn),
        }
    }
}

impl NodeWrapper {
    fn as_ref(&self) -> NodeRef<'_> {
        match self {
            Self::Leaf(text) => NodeRef::Node(Node::Leaf(text)),
            Self::Inner(inner) => NodeRef::Node(Node::Inner(inner)),
            Self::Error(err) => NodeRef::Node(Node::Error(err)),
            Self::Warning(warn) => NodeRef::Warning(warn),
        }
    }
}

impl SyntaxNode {
    /// Get the underlying node, descending past warnings.
    fn node(&self) -> Node<'_> {
        let mut node_ref = self.0.as_ref();
        loop {
            match node_ref {
                NodeRef::Node(node) => break node,
                NodeRef::Warning(warn) => node_ref = warn.child.as_ref(),
            }
        }
    }

    /// Get an inner node mutably, descending past warnings.
    fn inner_mut(&mut self) -> Option<&mut InnerNode> {
        match &mut self.0 {
            NodeTop::Leaf(..) | NodeTop::Error(..) => None,
            NodeTop::Inner(inner, ..) => Some(Arc::make_mut(inner)),
            NodeTop::Warning(warn, ..) => {
                let mut wrapper = &mut Arc::make_mut(warn).child;
                loop {
                    match wrapper {
                        NodeWrapper::Leaf(_) | NodeWrapper::Error(_) => break None,
                        NodeWrapper::Inner(inner) => {
                            break Some(Arc::make_mut(inner));
                        }
                        NodeWrapper::Warning(warn) => {
                            wrapper = &mut Arc::make_mut(warn).child
                        }
                    }
                }
            }
        }
    }

    /// Get the span mutably.
    fn span_mut(&mut self) -> &mut Span {
        match &mut self.0 {
            NodeTop::Leaf(_, span, _)
            | NodeTop::Inner(_, span, _)
            | NodeTop::Error(_, span, _)
            | NodeTop::Warning(_, span, _) => span,
        }
    }

    /// Get the hints for an error or warning mutably.
    #[track_caller]
    fn hints_mut(&mut self) -> &mut EcoVec<EcoString> {
        match &mut self.0 {
            NodeTop::Leaf(..) | NodeTop::Inner(..) => {
                panic!("expected an error or warning")
            }
            NodeTop::Error(err, ..) => &mut Arc::make_mut(err).hints,
            NodeTop::Warning(warn, ..) => &mut Arc::make_mut(warn).hints,
        }
    }
}

impl SyntaxNode {
    /// Create a new leaf node.
    #[track_caller]
    pub fn leaf(kind: SyntaxKind, text: impl Into<EcoString>) -> Self {
        debug_assert!(!kind.is_error());
        Self(NodeTop::Leaf(text.into(), Span::detached(), kind))
    }

    /// Create a new inner node with children.
    #[track_caller]
    pub fn inner(kind: SyntaxKind, children: Vec<SyntaxNode>) -> Self {
        debug_assert!(!kind.is_error());
        Self(NodeTop::Inner(Arc::new(InnerNode::new(children)), Span::detached(), kind))
    }

    /// Create a new error node with a user-presentable message for the given
    /// text. Note that the message is the first argument, and the text causing
    /// the error is the second argument.
    pub fn error(message: impl Into<EcoString>, text: impl Into<EcoString>) -> Self {
        Self(NodeTop::Error(
            Arc::new(ErrorNode::new(message.into(), text.into())),
            Span::detached(),
            SyntaxKind::Error,
        ))
    }

    /// Add a warning message to an existing node.
    pub fn warn(&mut self, message: impl Into<EcoString>) {
        let kind = self.kind();
        let span = self.span();
        let child = match std::mem::take(self).0 {
            NodeTop::Leaf(text, ..) => NodeWrapper::Leaf(text),
            NodeTop::Inner(inner, ..) => NodeWrapper::Inner(inner),
            NodeTop::Error(err, ..) => NodeWrapper::Error(err),
            NodeTop::Warning(warn, ..) => NodeWrapper::Warning(warn),
        };
        let warn = Arc::new(WarningWrapper::new(child, message.into()));
        *self = Self(NodeTop::Warning(warn, span, kind));
    }

    /// Add a user-presentable hint to an existing error or warning. Panics if
    /// this is not an error or warning.
    #[track_caller]
    pub fn hint(&mut self, hint: impl Into<EcoString>) {
        self.hints_mut().push(hint.into());
    }

    /// Add mutliple hints while building an error or warning. Panics if
    /// this is not an error or warning.
    #[track_caller]
    pub fn with_hints(mut self, hints: impl IntoIterator<Item = EcoString>) -> Self {
        self.hints_mut().extend(hints);
        self
    }

    /// Create a dummy node of the given kind.
    ///
    /// Panics if `kind` is `SyntaxKind::Error`.
    #[track_caller]
    pub const fn placeholder(kind: SyntaxKind) -> Self {
        if matches!(kind, SyntaxKind::Error) {
            panic!("cannot create error placeholder");
        }
        Self(NodeTop::Leaf(EcoString::new(), Span::detached(), kind))
    }

    /// The type of the node.
    pub fn kind(&self) -> SyntaxKind {
        match self.0 {
            NodeTop::Leaf(_, _, kind)
            | NodeTop::Inner(_, _, kind)
            | NodeTop::Error(_, _, kind)
            | NodeTop::Warning(_, _, kind) => kind,
        }
    }

    /// Return `true` if the length is 0.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The byte length of the node in the source text.
    pub fn len(&self) -> usize {
        match self.node() {
            Node::Leaf(text) => text.len(),
            Node::Inner(inner) => inner.len,
            Node::Error(err) => err.text.len(),
        }
    }

    /// The span of the node.
    pub fn span(&self) -> Span {
        match self.0 {
            NodeTop::Leaf(_, span, _)
            | NodeTop::Inner(_, span, _)
            | NodeTop::Error(_, span, _)
            | NodeTop::Warning(_, span, _) => span,
        }
    }

    /// The text of the node if it is a leaf or error node.
    ///
    /// Returns the empty string if this is an inner node.
    pub fn text(&self) -> &EcoString {
        static EMPTY: EcoString = EcoString::new();
        match self.node() {
            Node::Leaf(text) => text,
            Node::Inner(_) => &EMPTY,
            Node::Error(err) => &err.text,
        }
    }

    /// Extract the text from the node.
    ///
    /// Builds the string if this is an inner node.
    pub fn into_text(self) -> EcoString {
        // This isn't fully efficient for warnings, but the efficient
        // version is more complicated to read/write due to partial moves.
        match self.0 {
            NodeTop::Leaf(text, ..) => text,
            NodeTop::Error(err, ..) => err.text.clone(),
            NodeTop::Inner(..) | NodeTop::Warning(..) => {
                let mut buffer = EcoString::with_capacity(self.len());
                self.traverse(|node| {
                    match node.node() {
                        Node::Leaf(text) => buffer.push_str(text),
                        Node::Inner(_) => {}
                        Node::Error(err) => buffer.push_str(&err.text),
                    }
                    node.children()
                });
                buffer
            }
        }
    }

    /// The node's children.
    pub fn children(&self) -> std::slice::Iter<'_, SyntaxNode> {
        match self.node() {
            Node::Leaf(_) | Node::Error(_) => [].iter(),
            Node::Inner(inner, ..) => inner.children.iter(),
        }
    }

    /// Whether the node has diagnostic errors and/or warnings in it or its
    /// children. [`Diagnosis`] has public fields, so you can write
    /// `node.diagnosis().errors` to determine if a node is erroneous.
    ///
    /// This can be used to determine whether [`Self::errors_and_warnings`] will
    /// return an empty vector without traversing the tree if it will not.
    pub fn diagnosis(&self) -> Diagnosis {
        let diagnosis = match self.node() {
            Node::Leaf(_) => Diagnosis::default(),
            Node::Inner(inner) => inner.diagnosis,
            Node::Error(_) => Diagnosis { errors: true, warnings: false },
        };
        match &self.0 {
            NodeTop::Warning(..) => {
                Diagnosis { warnings: true, errors: diagnosis.errors }
            }
            _ => diagnosis,
        }
    }

    /// The error and warning diagnostics for this node and its descendants.
    pub fn errors_and_warnings(&self) -> (Vec<SyntaxDiagnostic>, Vec<SyntaxDiagnostic>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        self.traverse(|node| {
            let span = node.span();
            let mut node_ref = node.0.as_ref();
            loop {
                match node_ref {
                    NodeRef::Node(Node::Inner(inner)) if inner.diagnosis.either() => {
                        break inner.children.iter();
                    }
                    NodeRef::Node(Node::Leaf(_) | Node::Inner(_)) => {
                        break [].iter();
                    }
                    NodeRef::Node(Node::Error(err)) => {
                        errors.push(err.diagnostic(span));
                        break [].iter();
                    }
                    NodeRef::Warning(warn) => {
                        warnings.push(warn.diagnostic(span));
                        node_ref = warn.child.as_ref();
                    }
                }
            }
        });
        (errors, warnings)
    }

    /// Set a synthetic span for the node and all its descendants.
    pub fn synthesize(&mut self, span: Span) {
        self.synthesize_with(0, &mut |_| span);
    }

    /// Set a raw range span for each node.
    ///
    /// The range is determined by mapping the node's ranges through the given
    /// `mapper`.
    pub fn synthesize_mapped(&mut self, id: FileId, mapper: &RangeMapper) {
        self.synthesize_with(0, &mut |range| match mapper.map(range.clone()) {
            Some(mapped) => Span::from_range(id, mapped),
            None => {
                eprintln!("None, range: {range:?}");
                Span::detached()
            }
        });
    }

    /// Set a custom span for each node gives its range.
    ///
    /// Should be called with `offset = 0` on the root node.
    fn synthesize_with(
        &mut self,
        mut offset: usize,
        f: &mut impl FnMut(Range<usize>) -> Span,
    ) {
        let span = f(offset..offset + self.len());
        *self.span_mut() = span;
        if let Some(inner) = self.inner_mut() {
            inner.upper = span.number();
            for child in &mut inner.children {
                child.synthesize_with(offset, f);
                offset += child.len();
            }
        }
    }

    /// Whether the two syntax nodes are the same apart from spans.
    pub fn spanless_eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && {
            let mut ref_a = self.0.as_ref();
            let mut ref_b = other.0.as_ref();
            loop {
                match (ref_a, ref_b) {
                    (NodeRef::Node(a), NodeRef::Node(b)) => {
                        break match (a, b) {
                            (Node::Leaf(a), Node::Leaf(b)) => a == b,
                            (Node::Inner(a), Node::Inner(b)) => a.spanless_eq(b),
                            (Node::Error(a), Node::Error(b)) => a == b,
                            _ => false,
                        };
                    }
                    (NodeRef::Warning(a), NodeRef::Warning(b))
                        if a.message == b.message && a.hints == b.hints =>
                    {
                        ref_a = a.child.as_ref();
                        ref_b = b.child.as_ref();
                    }
                    _ => break false,
                }
            }
        }
    }
}

impl SyntaxNode {
    /// Convert the child to another kind.
    ///
    /// Don't use this for converting to an error!
    #[track_caller]
    pub(super) fn convert_to_kind(&mut self, new_kind: SyntaxKind) {
        debug_assert!(!new_kind.is_error());
        if self.kind().is_error() {
            // This handles warnings wrappping error nodes.
            panic!("cannot convert error");
        }
        match &mut self.0 {
            NodeTop::Leaf(_, _, kind)
            | NodeTop::Inner(_, _, kind)
            | NodeTop::Warning(_, _, kind) => *kind = new_kind,
            NodeTop::Error(..) => unreachable!(),
        }
    }

    /// Convert the child to an error, if it isn't already one.
    pub(super) fn convert_to_error(&mut self, message: impl Into<EcoString>) {
        if !self.kind().is_error() {
            let text = std::mem::take(self).into_text();
            *self = SyntaxNode::error(message.into(), text);
        }
    }

    /// Convert the child to an error stating that the given thing was
    /// expected, but the current kind was found.
    pub(super) fn expected(&mut self, expected: &str) {
        let kind = self.kind();
        self.convert_to_error(eco_format!("expected {expected}, found {}", kind.name()));
        if kind.is_keyword() && matches!(expected, "identifier" | "pattern") {
            self.hint(eco_format!(
                "keyword `{text}` is not allowed as an identifier; try `{text}_` instead",
                text = self.text(),
            ));
        }
    }

    /// Convert the child to an error stating it was unexpected.
    pub(super) fn unexpected(&mut self) {
        self.convert_to_error(eco_format!("unexpected {}", self.kind().name()));
    }

    /// Assign spans to each node.
    pub(super) fn numberize(
        &mut self,
        id: FileId,
        within: Range<u64>,
    ) -> NumberingResult<()> {
        if within.start >= within.end {
            return Err(Unnumberable);
        }

        let mid = Span::from_number(id, (within.start + within.end) / 2).unwrap();
        if let Some(inner) = self.inner_mut() {
            if let Some(new_span) = inner.numberize(id, None, within)? {
                *self.span_mut() = new_span;
            }
        } else {
            *self.span_mut() = mid;
        }

        Ok(())
    }

    /// Traverse the tree in-order, calling `f` on each node and recursing on
    /// the returned nodes. Note that `f` can prune the traversal at any point
    /// by yielding `[].iter()` instead of the actual children slice of an inner
    /// node.
    fn traverse(&self, mut f: impl FnMut(&Self) -> std::slice::Iter<'_, Self>) {
        fn recursive_step(
            node: &SyntaxNode,
            f: &mut impl FnMut(&SyntaxNode) -> std::slice::Iter<'_, SyntaxNode>,
        ) {
            for child in f(node) {
                recursive_step(child, f);
            }
        }
        // We pass in `&mut impl FnMut` so our caller doesn't have to.
        recursive_step(self, &mut f);
    }

    /// Whether this is a leaf node.
    pub(super) fn is_leaf(&self) -> bool {
        matches!(self.node(), Node::Leaf(_))
        // TODO: Should we also treat non-empty errors as leaves?
    }

    /// Whether this is an inner node.
    pub(super) fn is_inner(&self) -> bool {
        matches!(self.node(), Node::Inner(_))
    }

    /// The number of descendants, including the node itself.
    pub(super) fn descendants(&self) -> usize {
        match self.node() {
            Node::Leaf(_) | Node::Error(_) => 1,
            Node::Inner(inner) => inner.descendants,
        }
    }

    /// The node's children, mutably.
    pub(super) fn children_mut(&mut self) -> &mut [SyntaxNode] {
        if let Some(inner) = self.inner_mut() { &mut inner.children } else { &mut [] }
    }

    /// Replaces a range of children with a replacement.
    ///
    /// May have mutated the children if it returns `Err(_)`.
    pub(super) fn replace_children(
        &mut self,
        range: Range<usize>,
        replacement: Vec<SyntaxNode>,
    ) -> NumberingResult<()> {
        let span = self.span();
        if let Some(inner) = self.inner_mut()
            && let Some(new) = inner.replace_children(span, range, replacement)?
        {
            *self.span_mut() = new;
        }
        Ok(())
    }

    /// Update this node after changes were made to one of its children.
    pub(super) fn update_parent(
        &mut self,
        prev_len: usize,
        new_len: usize,
        prev_descendants: usize,
        new_descendants: usize,
    ) {
        if let Some(inner) = self.inner_mut() {
            inner.update_parent(prev_len, new_len, prev_descendants, new_descendants)
        }
    }

    /// The upper bound of assigned numbers in this subtree.
    pub(super) fn upper(&self) -> u64 {
        let span = self.span();
        match self.node() {
            Node::Leaf(_) | Node::Error(_) => span.number() + 1,
            Node::Inner(inner) => inner.upper,
        }
    }
}

impl Debug for SyntaxNode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        /// This helper lets us output `hint: "msg"` instead of `"hint: msg"`
        /// while using `debug_set` with warnings.
        /// FUTURE: In Rust 1.93, we can use `fmt::from_fn` instead!
        struct FieldHelper<'a>(&'static str, &'a EcoString);
        impl Debug for FieldHelper<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                write!(f, "{}: {:?}", self.0, self.1)
            }
        }

        struct Pair<'a>(NodeRef<'a>, SyntaxKind);
        impl Debug for Pair<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                let kind = self.1;
                match self.0 {
                    NodeRef::Node(node) => match node {
                        Node::Leaf(text) => {
                            write!(f, "{kind:?}: {text:?}")
                        }
                        Node::Inner(inner) => {
                            write!(f, "{kind:?}: {}", inner.len)?;
                            if !inner.children.is_empty() {
                                f.write_str(" ")?;
                                f.debug_list().entries(&inner.children).finish()?;
                            }
                            Ok(())
                        }
                        Node::Error(err) => err.fmt(f),
                    },
                    NodeRef::Warning(warn) => {
                        write!(f, "Warning: ")?;
                        // Use `debug_set` instead of `debug_struct` so we don't
                        // have to add a field name when outputting the child.
                        let mut out = f.debug_set();
                        out.entry(&FieldHelper("message", &warn.message));
                        for hint in &warn.hints {
                            out.entry(&FieldHelper("hint", hint));
                        }
                        out.entry(&Pair(warn.child.as_ref(), kind));
                        out.finish()
                    }
                }
            }
        }

        Pair(self.0.as_ref(), self.kind()).fmt(f)
    }
}

impl Default for SyntaxNode {
    fn default() -> Self {
        Self::leaf(SyntaxKind::End, EcoString::new())
    }
}

/// An inner node in the untyped syntax tree.
#[derive(Clone, Eq, PartialEq, Hash)]
struct InnerNode {
    /// The byte length of the node in the source.
    len: usize,
    /// The number of nodes in the whole subtree, including this node.
    descendants: usize,
    /// Whether this node or any of its children contain an error/warning
    /// diagnostic.
    diagnosis: Diagnosis,
    /// The upper bound of this node's numbering range.
    upper: u64,
    /// This node's children, losslessly make up this node.
    children: Vec<SyntaxNode>,
}

impl InnerNode {
    /// Create a new inner node with the given kind and children.
    fn new(children: Vec<SyntaxNode>) -> Self {
        let mut len = 0;
        let mut descendants = 1;
        let mut diagnosis = Diagnosis::default();

        for child in &children {
            len += child.len();
            descendants += child.descendants();
            diagnosis = diagnosis.or(child.diagnosis());
        }

        Self { len, descendants, diagnosis, upper: 0, children }
    }

    /// Assign span numbers `within` an interval to this node's subtree or just
    /// a `range` of its children.
    fn numberize(
        &mut self,
        id: FileId,
        range: Option<Range<usize>>,
        within: Range<u64>,
    ) -> NumberingResult<Option<Span>> {
        // Determine how many nodes we will number.
        let descendants = match &range {
            Some(range) if range.is_empty() => return Ok(None),
            Some(range) => self.children[range.clone()]
                .iter()
                .map(SyntaxNode::descendants)
                .sum::<usize>(),
            None => self.descendants,
        };

        // Determine the distance between two neighbouring assigned numbers. If
        // possible, we try to fit all numbers into the left half of `within`
        // so that there is space for future insertions.
        let space = within.end - within.start;
        let mut stride = space / (2 * descendants as u64);
        if stride == 0 {
            stride = space / self.descendants as u64;
            if stride == 0 {
                return Err(Unnumberable);
            }
        }

        // Number the node itself.
        let mut start = within.start;
        let mut span = None;
        if range.is_none() {
            let end = start + stride;
            span = Some(Span::from_number(id, (start + end) / 2).unwrap());
            self.upper = within.end;
            start = end;
        }

        // Number the children.
        let len = self.children.len();
        for child in &mut self.children[range.unwrap_or(0..len)] {
            let end = start + child.descendants() as u64 * stride;
            child.numberize(id, start..end)?;
            start = end;
        }

        Ok(span)
    }

    /// Whether the two inner nodes are the same apart from spans.
    fn spanless_eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self.descendants == other.descendants
            && self.diagnosis == other.diagnosis
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(a, b)| a.spanless_eq(b))
    }

    /// Replaces a range of children with a replacement.
    ///
    /// May have mutated the children if it returns `Err(_)`.
    fn replace_children(
        &mut self,
        span: Span,
        mut range: Range<usize>,
        replacement: Vec<SyntaxNode>,
    ) -> NumberingResult<Option<Span>> {
        let Some(id) = span.id() else { return Err(Unnumberable) };
        let mut replacement_range = 0..replacement.len();

        // Trim off common prefix.
        while range.start < range.end
            && replacement_range.start < replacement_range.end
            && self.children[range.start]
                .spanless_eq(&replacement[replacement_range.start])
        {
            range.start += 1;
            replacement_range.start += 1;
        }

        // Trim off common suffix.
        while range.start < range.end
            && replacement_range.start < replacement_range.end
            && self.children[range.end - 1]
                .spanless_eq(&replacement[replacement_range.end - 1])
        {
            range.end -= 1;
            replacement_range.end -= 1;
        }

        let mut replacement_vec = replacement;
        let replacement = &replacement_vec[replacement_range.clone()];
        let superseded = &self.children[range.clone()];

        // Compute the new byte length.
        self.len = self.len + replacement.iter().map(SyntaxNode::len).sum::<usize>()
            - superseded.iter().map(SyntaxNode::len).sum::<usize>();

        // Compute the new number of descendants.
        self.descendants = self.descendants
            + replacement.iter().map(SyntaxNode::descendants).sum::<usize>()
            - superseded.iter().map(SyntaxNode::descendants).sum::<usize>();

        // Update our diagnosis after the replacement.
        // - If we had no errors/warnings before, we can just use the replaced
        //   diagnosis
        // - Or, if our replacement has errors _and_ warnings, we can use that
        // - Otherwise, we need to update based on all of the children _outside_
        //   the replaced range in case we replaced the erroneous children
        let replaced_diagnosis = Diagnosis::any(replacement);
        if !self.diagnosis.either() || replaced_diagnosis.both() {
            self.diagnosis = replaced_diagnosis;
        } else {
            self.diagnosis = replaced_diagnosis.or(Diagnosis::or(
                Diagnosis::any(&self.children[..range.start]),
                Diagnosis::any(&self.children[range.end..]),
            ));
        }

        // Perform the replacement.
        self.children
            .splice(range.clone(), replacement_vec.drain(replacement_range.clone()));
        range.end = range.start + replacement_range.len();

        // Renumber the new children. Retries until it works, taking
        // exponentially more children into account.
        let mut left = 0;
        let mut right = 0;
        let max_left = range.start;
        let max_right = self.children.len() - range.end;
        loop {
            let renumber = range.start - left..range.end + right;

            // The minimum assignable number is either
            // - the upper bound of the node right before the to-be-renumbered
            //   children,
            // - or this inner node's span number plus one if renumbering starts
            //   at the first child.
            let start_number = renumber
                .start
                .checked_sub(1)
                .and_then(|i| self.children.get(i))
                .map_or(span.number() + 1, |child| child.upper());

            // The upper bound for renumbering is either
            // - the span number of the first child after the to-be-renumbered
            //   children,
            // - or this node's upper bound if renumbering ends behind the last
            //   child.
            let end_number = self
                .children
                .get(renumber.end)
                .map_or(self.upper, |next| next.span().number());

            // Try to renumber.
            let within = start_number..end_number;
            if let Ok(maybe_span) = self.numberize(id, Some(renumber), within) {
                return Ok(maybe_span);
            }

            // If it didn't even work with all children, we give up.
            if left == max_left && right == max_right {
                return Err(Unnumberable);
            }

            // Exponential expansion to both sides.
            left = (left + 1).next_power_of_two().min(max_left);
            right = (right + 1).next_power_of_two().min(max_right);
        }
    }

    /// Update this node after changes were made to one of its children.
    fn update_parent(
        &mut self,
        prev_len: usize,
        new_len: usize,
        prev_descendants: usize,
        new_descendants: usize,
    ) {
        self.len = self.len + new_len - prev_len;
        self.descendants = self.descendants + new_descendants - prev_descendants;
        self.diagnosis = Diagnosis::any(&self.children);
    }
}

/// Whether a node has diagnostic errors and/or warnings in it or its children.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub struct Diagnosis {
    pub errors: bool,
    pub warnings: bool,
}

impl Diagnosis {
    /// Whether there were errors or warnings.
    pub fn either(self) -> bool {
        self.errors | self.warnings
    }

    /// Whether there were both errors and warnings.
    pub fn both(self) -> bool {
        self.errors & self.warnings
    }

    /// Apply the `OR` of both fields separately.
    pub fn or(mut self, other: Self) -> Self {
        self.errors |= other.errors;
        self.warnings |= other.warnings;
        self
    }

    /// Whether any node in the given slice has errors or warnings.
    fn any(slice: &[SyntaxNode]) -> Self {
        slice
            .iter()
            .map(SyntaxNode::diagnosis)
            .fold(Self::default(), Self::or)
    }
}

/// A syntactical error or warning. This is mainly used by converting it to a
/// `SourceDiagnostic` during evaluation.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SyntaxDiagnostic {
    /// `true` if the diagnostic is an error, `false` if it's a warning.
    pub is_error: bool,
    /// The span targeted by the diagnostic.
    pub span: Span,
    /// The main diagnostic message.
    pub message: EcoString,
    /// Additional hints to the user indicating how this issue could be avoided
    /// or worked around.
    pub hints: EcoVec<EcoString>,
}

/// An error node in the untyped syntax tree.
#[derive(Clone, Eq, PartialEq, Hash)]
struct ErrorNode {
    /// The source text of the node.
    text: EcoString,
    /// The error message.
    message: EcoString,
    /// Additional hints to the user indicating how this error could be avoided
    /// or worked around.
    hints: EcoVec<EcoString>,
}

impl ErrorNode {
    /// Create a new error node.
    fn new(message: EcoString, text: EcoString) -> Self {
        Self { text, message, hints: eco_vec![] }
    }

    /// Produce the syntax diagnostic for an error.
    fn diagnostic(&self, span: Span) -> SyntaxDiagnostic {
        SyntaxDiagnostic {
            is_error: true,
            span,
            message: self.message.clone(),
            hints: self.hints.clone(),
        }
    }
}

impl Debug for ErrorNode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.text.is_empty() && self.hints.is_empty() {
            write!(f, "Error: {:?}", self.message)
        } else {
            let mut out = f.debug_struct("Error:");
            out.field("text", &self.text);
            out.field("message", &self.message);
            for hint in &self.hints {
                out.field("hint", hint);
            }
            out.finish()
        }
    }
}

/// A warning message wrapped around a node in the tree.
///
/// Warnings transparently wrap another node and do not have spans or text of
/// their own. This means their child cannot be directly found or mutated, only
/// affected _through_ the warning, usually via the [`SyntaxNode::get`] and
/// [`SyntaxNode::get_mut`] methods.
#[derive(Clone, Eq, PartialEq, Hash)]
struct WarningWrapper {
    /// The wrapped syntax node.
    child: NodeWrapper,
    /// The warning message.
    message: EcoString,
    /// Additional hints to the user indicating how this warning could be
    /// avoided or worked around.
    hints: EcoVec<EcoString>,
}

impl WarningWrapper {
    /// Wrap an existing syntax node in a warning node.
    fn new(child: NodeWrapper, message: EcoString) -> Self {
        Self { child, message, hints: eco_vec![] }
    }

    /// Produce the syntax diagnostic for a warning.
    fn diagnostic(&self, span: Span) -> SyntaxDiagnostic {
        SyntaxDiagnostic {
            is_error: false,
            span,
            message: self.message.clone(),
            hints: self.hints.clone(),
        }
    }
}

/// A syntax node in a context.
///
/// Knows its exact offset in the file and provides access to its
/// children, parent and siblings.
///
/// **Note that all sibling and leaf accessors skip over trivia!**
#[derive(Clone)]
pub struct LinkedNode<'a> {
    /// The underlying syntax node.
    node: &'a SyntaxNode,
    /// The parent of this node.
    parent: Option<Rc<Self>>,
    /// The index of this node in its parent's children array.
    index: usize,
    /// This node's byte offset in the source file.
    offset: usize,
}

impl<'a> LinkedNode<'a> {
    /// Start a new traversal at a root node.
    pub fn new(root: &'a SyntaxNode) -> Self {
        Self { node: root, parent: None, index: 0, offset: 0 }
    }

    /// Get the contained syntax node.
    pub fn get(&self) -> &'a SyntaxNode {
        self.node
    }

    /// The index of this node in its parent's children list.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The absolute byte offset of this node in the source file.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The byte range of this node in the source file.
    pub fn range(&self) -> Range<usize> {
        self.offset..self.offset + self.node.len()
    }

    /// An iterator over this node's children.
    pub fn children(&self) -> LinkedChildren<'a> {
        LinkedChildren {
            parent: Rc::new(self.clone()),
            iter: self.node.children().enumerate(),
            front: self.offset,
            back: self.offset + self.len(),
        }
    }

    /// Find a descendant with the given span.
    pub fn find(&self, span: Span) -> Option<LinkedNode<'a>> {
        if self.span() == span {
            return Some(self.clone());
        }

        if self.is_inner() && self.span().number() < span.number() {
            // The parent of a subtree has a smaller span number than all of its
            // descendants. Therefore, we can bail out early if the target span's
            // number is smaller than our number.

            // Use `self.children()`, not `inner.children()` to preserve being
            // in a `LinkedNode`.
            let mut children = self.children().peekable();
            while let Some(child) = children.next() {
                // Every node in this child's subtree has a smaller span number than
                // the next sibling. Therefore we only need to recurse if the next
                // sibling's span number is larger than the target span's number.
                if children
                    .peek()
                    .is_none_or(|next| next.span().number() > span.number())
                    && let Some(found) = child.find(span)
                {
                    return Some(found);
                }
            }
        }

        None
    }

    /// Get the [`SyntaxMode`] we will be in when immediately after this node.
    ///
    /// Unlike some other `LinkedNode` methods, this does not treat all trivia
    /// the same: it returns `None` for both comments and the bodies of raw text
    /// and returns `Some` for whitespace (based on the parent's mode). The only
    /// other way this would return `None` is when inside a partial tree, i.e.
    /// one not rooted in `Markup`, `Math`, or `Code`.
    ///
    /// Also note that errors inherit the mode of their parent.
    pub fn mode_after(&self) -> Option<SyntaxMode> {
        match self.kind().mode_after() {
            ModeAfter::Known(mode) => Some(mode),
            // Comments and the bodies of raw text have no mode.
            ModeAfter::None => None,
            ModeAfter::Text if self.parent_kind() == Some(SyntaxKind::Raw) => None,
            ModeAfter::RawDelim if self.index == 0 => None,
            // Text not under raw is always markup.
            ModeAfter::Text => Some(SyntaxMode::Markup),
            // An opening dollar sign starts math mode.
            ModeAfter::Dollar if self.index == 0 => Some(SyntaxMode::Math),
            // Spaces at the left/right of an equation are still in math mode.
            ModeAfter::Space if self.parent_kind() == Some(SyntaxKind::Equation) => {
                Some(SyntaxMode::Math)
            }
            // The position after something embedded with a hash is still code.
            ModeAfter::Embeddable
                if self
                    .prev_sibling_with_trivia()
                    .is_some_and(|prev| prev.kind() == SyntaxKind::Hash) =>
            {
                Some(SyntaxMode::Code)
            }
            // Otherwise, we're simply based on our parent's mode.
            ModeAfter::Parent
            | ModeAfter::RawDelim
            | ModeAfter::Space
            | ModeAfter::Dollar
            | ModeAfter::Embeddable => self.parent_mode(),
        }
    }

    /// Get the [`SyntaxMode`] we will be in when immediately after the parent
    /// of this node.
    pub fn parent_mode(&self) -> Option<SyntaxMode> {
        self.parent().and_then(Self::mode_after)
    }
}

/// Access to parents and siblings.
impl LinkedNode<'_> {
    /// Get this node's parent.
    pub fn parent(&self) -> Option<&Self> {
        self.parent.as_deref()
    }

    /// Get the first previous non-trivia sibling node.
    pub fn prev_sibling(&self) -> Option<Self> {
        let parent = self.parent.as_ref()?;
        let children = parent.node.children().as_slice();
        let mut offset = self.offset;
        for (index, node) in children[..self.index].iter().enumerate().rev() {
            offset -= node.len();
            if !node.kind().is_trivia() {
                let parent = Some(parent.clone());
                return Some(Self { node, parent, index, offset });
            }
        }
        None
    }

    /// Get the first previous sibling node, including potential trivia.
    pub fn prev_sibling_with_trivia(&self) -> Option<Self> {
        let parent = self.parent.as_ref()?;
        let children = parent.node.children().as_slice();
        let (index, node) = children[..self.index].iter().enumerate().next_back()?;
        let offset = self.offset - node.len();
        let parent = Some(parent.clone());
        Some(Self { node, parent, index, offset })
    }

    /// Get the next non-trivia sibling node.
    pub fn next_sibling(&self) -> Option<Self> {
        let parent = self.parent.as_ref()?;
        let children = parent.node.children();
        let mut offset = self.offset + self.len();
        for (index, node) in children.enumerate().skip(self.index + 1) {
            if !node.kind().is_trivia() {
                let parent = Some(parent.clone());
                return Some(Self { node, parent, index, offset });
            }
            offset += node.len();
        }
        None
    }

    /// Get the next sibling node, including potential trivia.
    pub fn next_sibling_with_trivia(&self) -> Option<Self> {
        let parent = self.parent.as_ref()?;
        let children = parent.node.children();
        let (index, node) = children.enumerate().nth(self.index + 1)?;
        let offset = self.offset + self.len();
        let parent = Some(parent.clone());
        Some(Self { node, parent, index, offset })
    }

    /// Get the kind of this node's parent.
    pub fn parent_kind(&self) -> Option<SyntaxKind> {
        Some(self.parent()?.node.kind())
    }

    /// Get the kind of this node's first previous non-trivia sibling.
    pub fn prev_sibling_kind(&self) -> Option<SyntaxKind> {
        Some(self.prev_sibling()?.node.kind())
    }

    /// Get the kind of this node's next non-trivia sibling.
    pub fn next_sibling_kind(&self) -> Option<SyntaxKind> {
        Some(self.next_sibling()?.node.kind())
    }
}

/// Indicates whether the cursor is before the related byte index, or after.
#[derive(Debug, Clone)]
pub enum Side {
    Before,
    After,
}

/// Access to leaves.
impl LinkedNode<'_> {
    /// Get the rightmost non-trivia leaf before this node.
    pub fn prev_leaf(&self) -> Option<Self> {
        let mut node = self.clone();
        while let Some(prev) = node.prev_sibling() {
            if let Some(leaf) = prev.rightmost_leaf() {
                return Some(leaf);
            }
            node = prev;
        }
        self.parent()?.prev_leaf()
    }

    /// Find the leftmost contained non-trivia leaf.
    pub fn leftmost_leaf(&self) -> Option<Self> {
        if self.is_leaf() && !self.kind().is_trivia() && !self.kind().is_error() {
            return Some(self.clone());
        }

        for child in self.children() {
            if let Some(leaf) = child.leftmost_leaf() {
                return Some(leaf);
            }
        }

        None
    }

    /// Get the leaf immediately before the specified byte offset.
    fn leaf_before(&self, cursor: usize) -> Option<Self> {
        if self.node.children().len() == 0 && cursor <= self.offset + self.len() {
            return Some(self.clone());
        }

        let mut offset = self.offset;
        let count = self.node.children().len();
        for (i, child) in self.children().enumerate() {
            let len = child.len();
            if (offset < cursor && cursor <= offset + len)
                || (offset == cursor && i + 1 == count)
            {
                return child.leaf_before(cursor);
            }
            offset += len;
        }

        None
    }

    /// Get the leaf after the specified byte offset.
    fn leaf_after(&self, cursor: usize) -> Option<Self> {
        if self.node.children().len() == 0 && cursor < self.offset + self.len() {
            return Some(self.clone());
        }

        let mut offset = self.offset;
        for child in self.children() {
            let len = child.len();
            if offset <= cursor && cursor < offset + len {
                return child.leaf_after(cursor);
            }
            offset += len;
        }

        None
    }

    /// Get the leaf at the specified byte offset.
    pub fn leaf_at(&self, cursor: usize, side: Side) -> Option<Self> {
        match side {
            Side::Before => self.leaf_before(cursor),
            Side::After => self.leaf_after(cursor),
        }
    }

    /// Find the rightmost contained non-trivia leaf.
    pub fn rightmost_leaf(&self) -> Option<Self> {
        if self.is_leaf() && !self.kind().is_trivia() {
            return Some(self.clone());
        }

        for child in self.children().rev() {
            if let Some(leaf) = child.rightmost_leaf() {
                return Some(leaf);
            }
        }

        None
    }

    /// Get the leftmost non-trivia leaf after this node.
    pub fn next_leaf(&self) -> Option<Self> {
        let mut node = self.clone();
        while let Some(next) = node.next_sibling() {
            if let Some(leaf) = next.leftmost_leaf() {
                return Some(leaf);
            }
            node = next;
        }
        self.parent()?.next_leaf()
    }
}

impl Deref for LinkedNode<'_> {
    type Target = SyntaxNode;

    /// Dereference to a syntax node. Note that this shortens the lifetime, so
    /// you may need to use [`get()`](Self::get) instead in some situations.
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl Debug for LinkedNode<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.node.fmt(f)
    }
}

/// An iterator over the children of a linked node.
pub struct LinkedChildren<'a> {
    /// The parent whose children we're iterating.
    parent: Rc<LinkedNode<'a>>,
    /// The underlying syntax nodes and their indices.
    iter: std::iter::Enumerate<std::slice::Iter<'a, SyntaxNode>>,
    /// The byte offset of the next child's start.
    front: usize,
    /// The byte offset after the final child.
    back: usize,
}

impl<'a> Iterator for LinkedChildren<'a> {
    type Item = LinkedNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (index, node) = self.iter.next()?;
        let offset = self.front;
        self.front += node.len();
        Some(LinkedNode {
            node,
            parent: Some(self.parent.clone()),
            index,
            offset,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl DoubleEndedIterator for LinkedChildren<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (index, node) = self.iter.next_back()?;
        self.back -= node.len();
        Some(LinkedNode {
            node,
            parent: Some(self.parent.clone()),
            index,
            offset: self.back,
        })
    }
}

impl ExactSizeIterator for LinkedChildren<'_> {}

/// Result of numbering a node within an interval.
pub(super) type NumberingResult<T> = Result<T, Unnumberable>;

/// Indicates that a node cannot be numbered within a given interval.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) struct Unnumberable;

impl Display for Unnumberable {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.pad("cannot number within this interval")
    }
}

impl std::error::Error for Unnumberable {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    /// Test the debug output of a `SyntaxNode`.
    #[test]
    fn test_debug() {
        // A standard syntax tree:
        assert_eq!(
            format!("{:#?}", crate::parse("= Head <label>")),
            "\
Markup: 14 [
    Heading: 6 [
        HeadingMarker: \"=\",
        Space: \" \",
        Markup: 4 [
            Text: \"Head\",
        ],
    ],
    Space: \" \",
    Label: \"<label>\",
]"
        );
        // A basic syntax error:
        assert_eq!(
            format!("{:#?}", crate::parse("#")),
            "\
Markup: 1 [
    Hash: \"#\",
    Error: \"expected expression\",
]"
        );
        // A syntax error with multiple hints:
        assert_eq!(
            format!("{:#?}", crate::parse("##")),
            "\
Markup: 2 [
    Hash: \"#\",
    Error: {
        text: \"#\",
        message: \"the character `#` is not valid in code\",
        hint: \"the preceding hash is causing this to parse in code mode\",
        hint: \"try escaping the preceding hash: `\\\\#`\",
    },
]"
        );
        // A warning with a hint:
        assert_eq!(
            format!("{:#?}", crate::parse("**")),
            "\
Markup: 2 [
    Warning: {
        message: \"no text within stars\",
        hint: \"using multiple consecutive stars (e.g. **) has no additional effect\",
        Strong: 2 [
            Star: \"*\",
            Markup: 0,
            Star: \"*\",
        ],
    },
]"
        );
    }

    #[test]
    fn test_linked_node() {
        let source = Source::detached("#set text(12pt, red)");

        // Find "text" with Before.
        let node = LinkedNode::new(source.root()).leaf_at(7, Side::Before).unwrap();
        assert_eq!(node.offset(), 5);
        assert_eq!(node.text(), "text");

        // Find "text" with After.
        let node = LinkedNode::new(source.root()).leaf_at(7, Side::After).unwrap();
        assert_eq!(node.offset(), 5);
        assert_eq!(node.text(), "text");

        // Go back to "#set". Skips the space.
        let prev = node.prev_sibling().unwrap();
        assert_eq!(prev.offset(), 1);
        assert_eq!(prev.text(), "set");
    }

    #[test]
    fn test_linked_node_non_trivia_leaf() {
        let source = Source::detached("#set fun(12pt, red)");
        let leaf = LinkedNode::new(source.root()).leaf_at(6, Side::Before).unwrap();
        let prev = leaf.prev_leaf().unwrap();
        assert_eq!(leaf.text(), "fun");
        assert_eq!(prev.text(), "set");

        // Check position 9 with Before.
        let source = Source::detached("#let x = 10");
        let leaf = LinkedNode::new(source.root()).leaf_at(9, Side::Before).unwrap();
        let prev = leaf.prev_leaf().unwrap();
        let next = leaf.next_leaf().unwrap();
        assert_eq!(prev.text(), "=");
        assert_eq!(leaf.text(), " ");
        assert_eq!(next.text(), "10");

        // Check position 9 with After.
        let source = Source::detached("#let x = 10");
        let leaf = LinkedNode::new(source.root()).leaf_at(9, Side::After).unwrap();
        let prev = leaf.prev_leaf().unwrap();
        assert!(leaf.next_leaf().is_none());
        assert_eq!(prev.text(), "=");
        assert_eq!(leaf.text(), "10");
    }
}
