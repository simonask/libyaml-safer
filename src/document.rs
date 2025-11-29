use std::io::BufRead;

use crate::{
    AliasData, Anchors, DEFAULT_MAPPING_TAG, DEFAULT_SCALAR_TAG, DEFAULT_SEQUENCE_TAG, Emitter,
    Error, Event, EventData, MappingStyle, Mark, Parser, ParserInner, Result, ScalarStyle,
    SequenceStyle, TagDirective, VersionDirective,
};

/// The document structure.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Document {
    /// The document nodes.
    pub nodes: Vec<Node>,
    /// The version directive.
    pub version_directive: Option<VersionDirective>,
    /// The list of tag directives.
    pub tag_directives: Vec<TagDirective>,
    /// Is the document start indicator implicit?
    pub start_implicit: bool,
    /// Is the document end indicator implicit?
    pub end_implicit: bool,
    /// The beginning of the document.
    pub start_mark: Mark,
    /// The end of the document.
    pub end_mark: Mark,
}

/// The node structure.
#[derive(Clone, Default, Debug)]
#[non_exhaustive]
pub struct Node {
    /// The node type.
    pub data: NodeData,
    /// The node tag.
    pub tag: Option<String>,
    /// The beginning of the node.
    pub start_mark: Mark,
    /// The end of the node.
    pub end_mark: Mark,
}

/// Node types.
#[derive(Clone, Default, Debug)]
pub enum NodeData {
    /// An empty node.
    #[default]
    NoNode,
    /// A scalar node.
    Scalar {
        /// The scalar value.
        value: String,
        /// The scalar style.
        style: ScalarStyle,
    },
    /// A sequence node.
    Sequence {
        /// The stack of sequence items.
        items: Vec<NodeItem>,
        /// The sequence style.
        style: SequenceStyle,
    },
    /// A mapping node.
    Mapping {
        /// The stack of mapping pairs (key, value).
        pairs: Vec<NodePair>,
        /// The mapping style.
        style: MappingStyle,
    },
}

/// An element of a sequence node.
pub type NodeItem = i32;

/// An element of a mapping node.
#[derive(Copy, Clone, Default, Debug)]
#[non_exhaustive]
pub struct NodePair {
    /// The key of the element.
    pub key: i32,
    /// The value of the element.
    pub value: i32,
}

impl Document {
    /// Create a YAML document.
    pub fn new(
        version_directive: Option<VersionDirective>,
        tag_directives_in: &[TagDirective],
        start_implicit: bool,
        end_implicit: bool,
    ) -> Document {
        let nodes = Vec::with_capacity(16);
        let tag_directives = tag_directives_in.to_vec();

        Document {
            nodes,
            version_directive,
            tag_directives,
            start_implicit,
            end_implicit,
            start_mark: Mark::default(),
            end_mark: Mark::default(),
        }
    }

    /// Get a node of a YAML document.
    ///
    /// Returns the node object or `None` if `index` is out of range.
    pub fn get_node_mut(&mut self, index: i32) -> Option<&mut Node> {
        self.nodes.get_mut(index as usize - 1)
    }

    /// Get a node of a YAML document.
    ///
    /// Returns the node object or `None` if `index` is out of range.
    pub fn get_node(&self, index: i32) -> Option<&Node> {
        self.nodes.get(index as usize - 1)
    }

    /// Get the root of a YAML document node.
    ///
    /// The root object is the first object added to the document.
    ///
    /// An empty document produced by the parser signifies the end of a YAML stream.
    ///
    /// Returns the node object or `None` if the document is empty.
    pub fn get_root_node(&mut self) -> Option<&mut Node> {
        self.nodes.get_mut(0)
    }

    /// Create a SCALAR node and attach it to the document.
    ///
    /// The `style` argument may be ignored by the emitter.
    ///
    /// Returns the node id or 0 on error.
    #[must_use]
    pub fn add_scalar(&mut self, tag: Option<&str>, value: &str, style: ScalarStyle) -> i32 {
        let mark = Mark {
            index: 0_u64,
            line: 0_u64,
            column: 0_u64,
        };
        let tag = tag.unwrap_or(DEFAULT_SCALAR_TAG);
        let tag_copy = String::from(tag);
        let value_copy = String::from(value);
        let node = Node {
            data: NodeData::Scalar {
                value: value_copy,
                style,
            },
            tag: Some(tag_copy),
            start_mark: mark,
            end_mark: mark,
        };
        self.nodes.push(node);
        self.nodes.len() as i32
    }

    /// Create a SEQUENCE node and attach it to the document.
    ///
    /// The `style` argument may be ignored by the emitter.
    ///
    /// Returns the node id, which is a nonzero integer.
    #[must_use]
    pub fn add_sequence(&mut self, tag: Option<&str>, style: SequenceStyle) -> i32 {
        let mark = Mark {
            index: 0_u64,
            line: 0_u64,
            column: 0_u64,
        };

        let items = Vec::with_capacity(16);
        let tag = tag.unwrap_or(DEFAULT_SEQUENCE_TAG);
        let tag_copy = String::from(tag);
        let node = Node {
            data: NodeData::Sequence { items, style },
            tag: Some(tag_copy),
            start_mark: mark,
            end_mark: mark,
        };
        self.nodes.push(node);
        self.nodes.len() as i32
    }

    /// Create a MAPPING node and attach it to the document.
    ///
    /// The `style` argument may be ignored by the emitter.
    ///
    /// Returns the node id, which is a nonzero integer.
    #[must_use]
    pub fn add_mapping(&mut self, tag: Option<&str>, style: MappingStyle) -> i32 {
        let mark = Mark {
            index: 0_u64,
            line: 0_u64,
            column: 0_u64,
        };
        let pairs = Vec::with_capacity(16);
        let tag = tag.unwrap_or(DEFAULT_MAPPING_TAG);
        let tag_copy = String::from(tag);

        let node = Node {
            data: NodeData::Mapping { pairs, style },
            tag: Some(tag_copy),
            start_mark: mark,
            end_mark: mark,
        };

        self.nodes.push(node);
        self.nodes.len() as i32
    }

    /// Add an item to a SEQUENCE node.
    pub fn append_sequence_item(&mut self, sequence: i32, item: i32) {
        assert!(sequence > 0 && sequence as usize - 1 < self.nodes.len());
        assert!(matches!(
            &self.nodes[sequence as usize - 1].data,
            NodeData::Sequence { .. }
        ));
        assert!(item > 0 && item as usize - 1 < self.nodes.len());
        if let NodeData::Sequence { items, .. } = &mut self.nodes[sequence as usize - 1].data {
            items.push(item);
        }
    }

    /// Add a pair of a key and a value to a MAPPING node.
    pub fn yaml_document_append_mapping_pair(&mut self, mapping: i32, key: i32, value: i32) {
        assert!(mapping > 0 && mapping as usize - 1 < self.nodes.len());
        assert!(matches!(
            &self.nodes[mapping as usize - 1].data,
            NodeData::Mapping { .. }
        ));
        assert!(key > 0 && key as usize - 1 < self.nodes.len());
        assert!(value > 0 && value as usize - 1 < self.nodes.len());
        let pair = NodePair { key, value };
        if let NodeData::Mapping { pairs, .. } = &mut self.nodes[mapping as usize - 1].data {
            pairs.push(pair);
        }
    }

    /// Parse the input stream and produce the next YAML document.
    ///
    /// Call this function subsequently to produce a sequence of documents
    /// constituting the input stream.
    ///
    /// If the produced document has no root node, it means that the document
    /// end has been reached.
    ///
    /// An application must not alternate the calls of [`Document::load()`] with
    /// the calls of [`Parser::parse()`]. Doing this will break the parser.
    pub fn load<R: BufRead>(parser: &mut Parser<R>) -> Result<Document> {
        let mut document = Document::new(None, &[], false, false);
        document.nodes.reserve(16);

        if !parser.scanner.stream_start_produced {
            match parser.parse() {
                Ok(Event {
                    data: EventData::StreamStart { .. },
                    ..
                }) => (),
                Ok(_) => panic!("expected stream start"),
                Err(err) => {
                    parser.inner.delete_aliases();
                    return Err(err);
                }
            }
        }
        if parser.scanner.stream_end_produced {
            return Ok(document);
        }
        let err: Error;
        match parser.parse() {
            Ok(event) => {
                if let EventData::StreamEnd = &event.data {
                    return Ok(document);
                }
                parser.inner.aliases.reserve(16);
                match document.load_document(parser, event) {
                    Ok(()) => {
                        parser.inner.delete_aliases();
                        return Ok(document);
                    }
                    Err(e) => err = e,
                }
            }
            Err(e) => err = e,
        }
        parser.inner.delete_aliases();
        Err(err)
    }

    fn load_document<R: BufRead>(&mut self, parser: &mut Parser<R>, event: Event) -> Result<()> {
        let mut ctx = vec![];
        if let EventData::DocumentStart {
            version_directive,
            tag_directives,
            implicit,
        } = event.data
        {
            self.version_directive = version_directive;
            self.tag_directives = tag_directives;
            self.start_implicit = implicit;
            self.start_mark = event.start_mark;
            ctx.reserve(16);
            if let Err(err) = self.load_nodes(parser, &mut ctx) {
                ctx.clear();
                return Err(err);
            }
            ctx.clear();
            Ok(())
        } else {
            panic!("Expected YAML_DOCUMENT_START_EVENT")
        }
    }

    fn load_nodes<R: BufRead>(&mut self, parser: &mut Parser<R>, ctx: &mut Vec<i32>) -> Result<()> {
        let end_implicit;
        let end_mark;

        loop {
            let event = parser.parse()?;
            match event.data {
                EventData::StreamStart { .. } => panic!("unexpected stream start event"),
                EventData::StreamEnd => panic!("unexpected stream end event"),
                EventData::DocumentStart { .. } => panic!("unexpected document start event"),
                EventData::DocumentEnd { implicit } => {
                    end_implicit = implicit;
                    end_mark = event.end_mark;
                    break;
                }
                EventData::Alias { .. } => {
                    self.load_alias(&parser.inner, event, ctx)?;
                }
                EventData::Scalar { .. } => {
                    self.load_scalar(&mut parser.inner, event, ctx)?;
                }
                EventData::SequenceStart { .. } => {
                    self.load_sequence(&mut parser.inner, event, ctx)?;
                }
                EventData::SequenceEnd => {
                    self.load_sequence_end(event, ctx)?;
                }
                EventData::MappingStart { .. } => {
                    self.load_mapping(&mut parser.inner, event, ctx)?;
                }
                EventData::MappingEnd => {
                    self.load_mapping_end(event, ctx)?;
                }
            }
        }
        self.end_implicit = end_implicit;
        self.end_mark = end_mark;
        Ok(())
    }

    fn register_anchor(
        &mut self,
        parser: &mut ParserInner,
        index: i32,
        anchor: Option<String>,
    ) -> Result<()> {
        let anchor = match anchor {
            Some(anchor) => anchor,
            None => return Ok(()),
        };
        let data = AliasData {
            anchor,
            index,
            mark: self.nodes[index as usize - 1].start_mark,
        };
        for alias_data in &parser.aliases {
            if alias_data.anchor == data.anchor {
                return Err(Error::composer(
                    "found duplicate anchor; first occurrence",
                    alias_data.mark,
                    "second occurrence",
                    data.mark,
                ));
            }
        }
        parser.aliases.push(data);
        Ok(())
    }

    fn load_node_add(&mut self, ctx: &[i32], index: i32) -> Result<()> {
        let parent_index = match ctx.last() {
            Some(parent_index) => parent_index,
            None => return Ok(()),
        };
        let parent_index = *parent_index;
        let parent = &mut self.nodes[parent_index as usize - 1];
        match parent.data {
            NodeData::Sequence { ref mut items, .. } => {
                items.push(index);
            }
            NodeData::Mapping { ref mut pairs, .. } => match pairs.last_mut() {
                // If the last pair does not have a value, set `index` as the value.
                Some(pair @ NodePair { value: 0, .. }) => {
                    pair.value = index;
                }
                // Otherwise push a new pair where `index` is the key.
                _ => pairs.push(NodePair {
                    key: index,
                    value: 0,
                }),
            },
            _ => {
                panic!("document parent node is not a sequence or a mapping")
            }
        }
        Ok(())
    }

    fn load_alias(&mut self, parser: &ParserInner, event: Event, ctx: &[i32]) -> Result<()> {
        let anchor = match &event.data {
            EventData::Alias { anchor } => anchor,
            _ => unreachable!(),
        };

        for alias_data in &parser.aliases {
            if alias_data.anchor == *anchor {
                return self.load_node_add(ctx, alias_data.index);
            }
        }

        Err(Error::composer(
            "",
            Mark::default(),
            "found undefined alias",
            event.start_mark,
        ))
    }

    fn load_scalar(&mut self, parser: &mut ParserInner, event: Event, ctx: &[i32]) -> Result<()> {
        let (mut tag, value, style, anchor) = match event.data {
            EventData::Scalar {
                tag,
                value,
                style,
                anchor,
                ..
            } => (tag, value, style, anchor),
            _ => unreachable!(),
        };

        if tag.is_none() || tag.as_deref() == Some("!") {
            tag = Some(String::from(DEFAULT_SCALAR_TAG));
        }
        let node = Node {
            data: NodeData::Scalar { value, style },
            tag,
            start_mark: event.start_mark,
            end_mark: event.end_mark,
        };
        self.nodes.push(node);
        let index: i32 = self.nodes.len() as i32;
        self.register_anchor(parser, index, anchor)?;
        self.load_node_add(ctx, index)
    }

    fn load_sequence(
        &mut self,
        parser: &mut ParserInner,
        event: Event,
        ctx: &mut Vec<i32>,
    ) -> Result<()> {
        let (anchor, mut tag, style) = match event.data {
            EventData::SequenceStart {
                anchor,
                tag,
                style,
                ..
            } => (anchor, tag, style),
            _ => unreachable!(),
        };

        let mut items = Vec::with_capacity(16);

        if tag.is_none() || tag.as_deref() == Some("!") {
            tag = Some(String::from(DEFAULT_SEQUENCE_TAG));
        }

        let node = Node {
            data: NodeData::Sequence {
                items: core::mem::take(&mut items),
                style,
            },
            tag,
            start_mark: event.start_mark,
            end_mark: event.end_mark,
        };

        self.nodes.push(node);
        let index: i32 = self.nodes.len() as i32;
        self.register_anchor(parser, index, anchor)?;
        self.load_node_add(ctx, index)?;
        ctx.push(index);
        Ok(())
    }

    fn load_sequence_end(&mut self, event: Event, ctx: &mut Vec<i32>) -> Result<()> {
        let index = match ctx.last().copied() {
            Some(index) => index,
            None => panic!("sequence_end without a current sequence"),
        };
        assert!(matches!(
            self.nodes[index as usize - 1].data,
            NodeData::Sequence { .. }
        ));
        self.nodes[index as usize - 1].end_mark = event.end_mark;
        ctx.pop();
        Ok(())
    }

    fn load_mapping(
        &mut self,
        parser: &mut ParserInner,
        event: Event,
        ctx: &mut Vec<i32>,
    ) -> Result<()> {
        let (anchor, mut tag, style) = match event.data {
            EventData::MappingStart {
                anchor,
                tag,
                style,
                ..
            } => (anchor, tag, style),
            _ => unreachable!(),
        };

        let mut pairs = Vec::with_capacity(16);

        if tag.is_none() || tag.as_deref() == Some("!") {
            tag = Some(String::from(DEFAULT_MAPPING_TAG));
        }
        let node = Node {
            data: NodeData::Mapping {
                pairs: core::mem::take(&mut pairs),
                style,
            },
            tag,
            start_mark: event.start_mark,
            end_mark: event.end_mark,
        };
        self.nodes.push(node);
        let index: i32 = self.nodes.len() as i32;
        self.register_anchor(parser, index, anchor)?;
        self.load_node_add(ctx, index)?;
        ctx.push(index);
        Ok(())
    }

    fn load_mapping_end(&mut self, event: Event, ctx: &mut Vec<i32>) -> Result<()> {
        let index = match ctx.last().copied() {
            Some(index) => index,
            None => panic!("mapping_end without a current mapping"),
        };
        assert!(matches!(
            self.nodes[index as usize - 1].data,
            NodeData::Mapping { .. }
        ));
        self.nodes[index as usize - 1].end_mark = event.end_mark;
        ctx.pop();
        Ok(())
    }

    /// Emit a YAML document.
    ///
    /// The document object may be generated using the [`Document::load()`]
    /// function or the [`Document::new()`] function.
    pub fn dump(mut self, emitter: &mut Emitter) -> Result<()> {
        if !emitter.opened {
            if let Err(err) = emitter.open() {
                emitter.reset_anchors();
                return Err(err);
            }
        }
        if self.nodes.is_empty() {
            // TODO: Do we really want to close the emitter just because the
            // document contains no nodes? Isn't it OK to emit multiple documents in
            // the same stream?
            emitter.close()?;
        } else {
            assert!(emitter.opened);
            emitter.anchors = vec![Anchors::default(); self.nodes.len()];
            let event = Event::new(EventData::DocumentStart {
                version_directive: self.version_directive,
                tag_directives: core::mem::take(&mut self.tag_directives),
                implicit: self.start_implicit,
            });
            emitter.emit(event)?;
            self.anchor_node(emitter, 1);
            self.dump_node(emitter, 1)?;
            let event = Event::document_end(self.end_implicit);
            emitter.emit(event)?;
        }

        emitter.reset_anchors();
        Ok(())
    }

    fn anchor_node(&self, emitter: &mut Emitter, index: i32) {
        let node = &self.nodes[index as usize - 1];
        emitter.anchors[index as usize - 1].references += 1;
        if emitter.anchors[index as usize - 1].references == 1 {
            match &node.data {
                NodeData::Sequence { items, .. } => {
                    for item in items {
                        emitter.anchor_node_sub(*item);
                    }
                }
                NodeData::Mapping { pairs, .. } => {
                    for pair in pairs {
                        emitter.anchor_node_sub(pair.key);
                        emitter.anchor_node_sub(pair.value);
                    }
                }
                _ => {}
            }
        } else if emitter.anchors[index as usize - 1].references == 2 {
            emitter.last_anchor_id += 1;
            emitter.anchors[index as usize - 1].anchor = emitter.last_anchor_id;
        }
    }

    fn dump_node(&mut self, emitter: &mut Emitter, index: i32) -> Result<()> {
        assert!(index > 0);
        let node = &mut self.nodes[index as usize - 1];
        let anchor_id: i32 = emitter.anchors[index as usize - 1].anchor;
        let mut anchor: Option<String> = None;
        if anchor_id != 0 {
            anchor = Some(Emitter::generate_anchor(anchor_id));
        }
        if emitter.anchors[index as usize - 1].serialized {
            return Self::dump_alias(emitter, anchor.unwrap());
        }
        emitter.anchors[index as usize - 1].serialized = true;

        let node = core::mem::take(node);
        match node.data {
            NodeData::Scalar { .. } => Self::dump_scalar(emitter, node, anchor),
            NodeData::Sequence { .. } => self.dump_sequence(emitter, node, anchor),
            NodeData::Mapping { .. } => self.dump_mapping(emitter, node, anchor),
            _ => unreachable!("document node is neither a scalar, sequence, or a mapping"),
        }
    }

    fn dump_alias(emitter: &mut Emitter, anchor: String) -> Result<()> {
        let event = Event::new(EventData::Alias { anchor });
        emitter.emit(event)
    }

    fn dump_scalar(emitter: &mut Emitter, node: Node, anchor: Option<String>) -> Result<()> {
        let plain_implicit = node.tag.as_deref() == Some(DEFAULT_SCALAR_TAG);
        let quoted_implicit = node.tag.as_deref() == Some(DEFAULT_SCALAR_TAG); // TODO: Why compare twice?! (even the C code does this)

        let (value, style) = match node.data {
            NodeData::Scalar { value, style } => (value, style),
            _ => unreachable!(),
        };
        let event = Event::new(EventData::Scalar {
            anchor,
            tag: node.tag,
            value,
            plain_implicit,
            quoted_implicit,
            style,
        });
        emitter.emit(event)
    }

    fn dump_sequence(
        &mut self,
        emitter: &mut Emitter,
        node: Node,
        anchor: Option<String>,
    ) -> Result<()> {
        let implicit = node.tag.as_deref() == Some(DEFAULT_SEQUENCE_TAG);

        let (items, style) = match node.data {
            NodeData::Sequence { items, style } => (items, style),
            _ => unreachable!(),
        };
        let event = Event::new(EventData::SequenceStart {
            anchor,
            tag: node.tag,
            implicit,
            style,
        });

        emitter.emit(event)?;
        for item in items {
            self.dump_node(emitter, item)?;
        }
        let event = Event::sequence_end();
        emitter.emit(event)
    }

    fn dump_mapping(
        &mut self,
        emitter: &mut Emitter,
        node: Node,
        anchor: Option<String>,
    ) -> Result<()> {
        let implicit = node.tag.as_deref() == Some(DEFAULT_MAPPING_TAG);

        let (pairs, style) = match node.data {
            NodeData::Mapping { pairs, style } => (pairs, style),
            _ => unreachable!(),
        };
        let event = Event::new(EventData::MappingStart {
            anchor,
            tag: node.tag,
            implicit,
            style,
        });

        emitter.emit(event)?;
        for pair in pairs {
            self.dump_node(emitter, pair.key)?;
            self.dump_node(emitter, pair.value)?;
        }
        let event = Event::mapping_end();
        emitter.emit(event)
    }
}
