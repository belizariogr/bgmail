use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use parley::{
    FontContext, InlineBox, InlineBoxKind, LayoutContext, LineHeight, TreeBuilder,
    WhiteSpaceCollapse,
};
use style::{
    computed_values::position::T as PositionProperty,
    values::computed::BorderStyle,
    values::{
        computed::{Display, Float},
        specified::box_::{DisplayInside, DisplayOutside},
    },
};
use stylo_taffy::CopyStyleGetters;

use crate::{
    NodeData,
    layout::list::BULLET_FONT_FAMILY,
    layout::replaced::is_inline_box_element,
    layout::text_transform::{CaseTransform, TextTransformer},
    node::{ListItemLayout, ListItemLayoutPosition, Marker, TextBrush, TextLayout},
    stylo_to_parley,
};

pub(crate) fn build_inline_layout_into(
    nodes: &crate::NodeTree,
    layout_ctx: &mut LayoutContext<TextBrush>,
    font_ctx: &mut FontContext,
    text_layout: &mut TextLayout,
    scale: f32,
    inline_context_root_node_id: NodeId,
    line_height_quirk: bool,
) {
    // Get the inline context's root node's text styles
    let root_node = &nodes[inline_context_root_node_id];
    let root_node_style = root_node.primary_styles().or_else(|| {
        root_node
            .parent
            .and_then(|parent_id| nodes[parent_id].primary_styles())
    });

    // BGMail: an anonymous block wrapping the inline content of an inline
    // element that also contains blocks (`<font size=1>text<p>..</p></font>`)
    // inherits, per CSS 2.1 §9.2.1.1, from the enclosing *block*: that block
    // gives the line its strut. The inline ancestors still style their text,
    // so they become style spans around the content (outermost first).
    let mut inline_ancestors = Vec::new();
    let mut strut_style = None;
    if root_node.is_anonymous() {
        let mut ancestor = root_node.parent;
        while let Some(id) = ancestor {
            let node = &nodes[id];
            if node
                .display_style()
                .is_some_and(|display| display.is_inline_flow())
            {
                inline_ancestors.push(id);
                ancestor = node.parent;
            } else {
                if !inline_ancestors.is_empty() {
                    strut_style = node.primary_styles().map(|style| (id, style));
                }
                break;
            }
        }
    }
    let mut parley_style = match &strut_style {
        Some((block_id, style)) => stylo_to_parley::style(*block_id, style),
        None => root_node_style
            .as_ref()
            .map(|s| stylo_to_parley::style(inline_context_root_node_id, s))
            .unwrap_or_else(|| parley::TextStyle {
                white_space_collapse: WhiteSpaceCollapse::Collapse,
                ..Default::default()
            }),
    };
    // BGMail: the block's own `vertical-align` (a cell's `middle`) aligns the
    // block, not its lines; as the root style it would shift every span that
    // inherits the root's style (`<td>a<br>b` lines grew by a pixel).
    parley_style.vertical_align = Default::default();
    // The block's strut only counts when the block itself holds text (with
    // inline ancestors, that text belongs to the innermost one).
    if line_height_quirk && (strut_style.is_some() || !has_direct_text(nodes, root_node)) {
        parley_style.line_height = LineHeight::Absolute(0.0);
    }

    // Create a parley tree builder
    let mut builder = layout_ctx.tree_builder(font_ctx, scale, true, &parley_style);
    let mut open_spans = 0;
    if strut_style.is_some() {
        for &id in inline_ancestors.iter().rev() {
            if let Some(style) = nodes[id].primary_styles() {
                let mut span = stylo_to_parley::style(id, &style);
                let text_node = if id == inline_ancestors[0] {
                    root_node
                } else {
                    &nodes[id]
                };
                if line_height_quirk && line_height_quirk_applies(nodes, text_node, &style) {
                    span.line_height = LineHeight::Absolute(0.0);
                }
                builder.push_style_span(span);
                open_spans += 1;
            }
        }
    }
    if let Some(style) = root_node_style.as_deref() {
        builder.set_base_direction(stylo_to_parley::base_direction(
            style.clone_direction(),
            style.clone_unicode_bidi(),
        ));
    }

    let text_transform = root_node_style
        .as_deref()
        .map(|s| CaseTransform::from_style(s))
        .unwrap_or(CaseTransform::NONE);

    // Render position-inside list items
    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
    }) = root_node
        .element_data()
        .and_then(|el| el.list_item_data.as_deref())
    {
        match marker {
            // Bullet glyphs live in the bundled bullet font. The position-outside
            // path already asks for it; without the same span here a marker like
            // disclosure-closed (U+25B8) falls back to the element's own font and
            // renders as a missing glyph.
            Marker::Char(char) => {
                let mut marker_style = parley_style.clone();
                marker_style.font_family = BULLET_FONT_FAMILY.into();
                builder.push_style_span(marker_style);
                builder.push_text(&format!("{char} "));
                builder.pop_style_span();
            }
            Marker::String(str) => builder.push_text(str),
        }
    };
    // The marker is a separate box, so words in the content don't continue from it.
    let mut text_transformer = TextTransformer::default();
    text_transformer.word_break(&builder);

    let mut quirk = LineHeightQuirk {
        enabled: line_height_quirk,
        line_empty: true,
    };
    if let Some(before_id) = root_node.before() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            before_id,
            &text_transform,
            &mut quirk,
        );
    }
    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            child_id,
            &text_transform,
            &mut quirk,
        );
    }
    if let Some(after_id) = root_node.after() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            after_id,
            &text_transform,
            &mut quirk,
        );
    }

    for _ in 0..open_spans {
        builder.pop_style_span();
    }
    text_layout.text = builder.build_into(&mut text_layout.layout);
    return;

    fn build_inline_layout_recursive(
        builder: &mut TreeBuilder<TextBrush>,
        text_transformer: &mut TextTransformer,
        nodes: &crate::NodeTree,
        node_id: NodeId,
        parent_text_transform: &CaseTransform,
        quirk: &mut LineHeightQuirk,
    ) {
        let node = &nodes[node_id];

        let style = node.primary_styles();
        let style = style.as_ref();

        let text_transform = style
            .map(|s| CaseTransform::from_style(s))
            .unwrap_or(CaseTransform::NONE);

        match &node.data {
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                // if the input type is hidden, hide it
                if *element_data.name.local == *"input" {
                    if let Some("hidden") = element_data.attr(local_name!("type")) {
                        return;
                    }
                }

                let display = node.display_style().unwrap_or(Display::inline());
                let position = style
                    .map(|s| s.clone_position())
                    .unwrap_or(PositionProperty::Static);
                let float = style.map(|s| s.clone_float()).unwrap_or(Float::None);
                let box_kind = if position.is_absolutely_positioned() {
                    InlineBoxKind::OutOfFlow
                } else if float.is_floating() {
                    InlineBoxKind::CustomOutOfFlow
                } else {
                    InlineBoxKind::InFlow
                };

                match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::None) => {
                        // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                    }
                    (DisplayOutside::None, DisplayInside::Contents) => {
                        let whitespace = style.map(|s| {
                            [
                                parley::StyleProperty::WhiteSpaceCollapse(
                                    stylo_to_parley::white_space_collapse(
                                        s.clone_white_space_collapse(),
                                    ),
                                ),
                                parley::StyleProperty::TextWrapMode(
                                    stylo_to_parley::text_wrap_mode(s.clone_text_wrap_mode()),
                                ),
                            ]
                        });
                        builder.push_style_modification_span(
                            whitespace
                                .as_ref()
                                .map_or(&[][..], |styles| styles.as_slice()),
                        );
                        for child_id in node.children.iter().copied() {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            build_inline_layout_recursive(
                                builder,
                                text_transformer,
                                nodes,
                                child_id,
                                &text_transform,
                                quirk,
                            );
                        }
                        builder.pop_style_span();
                    }
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag_name = &element_data.name.local;

                        if is_inline_box_element(tag_name) {
                            if box_kind == InlineBoxKind::InFlow {
                                text_transformer.word_break(builder);
                            }
                            quirk.line_empty = false;
                            builder.push_inline_box(InlineBox {
                                id: node_id.as_u64(),
                                kind: box_kind,
                                // Overridden by push_inline_box method
                                index: 0,
                                // Width and height are set during layout
                                width: 0.0,
                                height: 0.0,
                                baseline: None,
                                vertical_align: node
                                    .primary_styles()
                                    .map(|s| stylo_to_parley::vertical_align(&s))
                                    .unwrap_or_default(),
                            });
                        } else if *tag_name == local_name!("br") {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            // TODO: update span id for br spans
                            match style.filter(|_| quirk.enabled && quirk.line_empty) {
                                // BGMail: under the line height quirk a break
                                // alone on its line is text: its own span gives
                                // that line the parent's line height (the
                                // parent may have none, holding no text).
                                Some(style) => builder.push_style_span(parley::TextStyle {
                                    white_space_collapse: WhiteSpaceCollapse::Preserve,
                                    ..stylo_to_parley::style(node.id, style)
                                }),
                                None => builder.push_style_modification_span(&[
                                    parley::StyleProperty::WhiteSpaceCollapse(
                                        WhiteSpaceCollapse::Preserve,
                                    ),
                                ]),
                            }
                            builder.push_text("\n");
                            builder.pop_style_span();
                            quirk.line_empty = true;
                            text_transformer.word_break(builder);
                        } else {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            let style = node
                                .primary_styles()
                                .map(|s| {
                                    let mut style = stylo_to_parley::style(node.id, &s);
                                    if quirk.enabled && line_height_quirk_applies(nodes, node, &s) {
                                        style.line_height = LineHeight::Absolute(0.0);
                                    }
                                    style
                                })
                                .unwrap_or_else(|| parley::TextStyle {
                                    white_space_collapse: WhiteSpaceCollapse::Collapse,
                                    ..Default::default()
                                });

                            builder.push_style_span(style);

                            // BGMail: reserve the inline element's horizontal margin,
                            // border and padding (Parley has no inline padding) with
                            // zero-height spacer boxes at its start and end.
                            let (start_space, end_space) = node
                                .primary_styles()
                                .map(|s| inline_edge_space(&s))
                                .unwrap_or((0.0, 0.0));
                            if start_space > 0.0 {
                                builder.push_inline_box(spacer_box(
                                    node_id.as_u64(),
                                    false,
                                    start_space,
                                ));
                            }

                            if let Some(before_id) = node.before() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    before_id,
                                    &text_transform,
                                    quirk,
                                );
                            }

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    child_id,
                                    &text_transform,
                                    quirk,
                                );
                            }
                            if let Some(after_id) = node.after() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    after_id,
                                    &text_transform,
                                    quirk,
                                );
                            }

                            if end_space > 0.0 {
                                builder.push_inline_box(spacer_box(
                                    node_id.as_u64(),
                                    true,
                                    end_space,
                                ));
                            }
                            builder.pop_style_span();
                        }
                    }
                    // Inline box
                    (_, _) => {
                        if box_kind == InlineBoxKind::InFlow {
                            text_transformer.word_break(builder);
                        }
                        quirk.line_empty = false;
                        builder.push_inline_box(InlineBox {
                            id: node_id.as_u64(),
                            kind: box_kind,
                            // Overridden by push_inline_box method
                            index: 0,
                            // Width and height are set during layout
                            width: 0.0,
                            height: 0.0,
                            baseline: None,
                            vertical_align: node
                                .primary_styles()
                                .map(|s| stylo_to_parley::vertical_align(&s))
                                .unwrap_or_default(),
                        });
                    }
                };
            }
            NodeData::Text(data) => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                // dbg!(&data.content);

                if !data.content.chars().all(|c| c.is_ascii_whitespace()) {
                    quirk.line_empty = false;
                }
                let text =
                    text_transformer.transform(&data.content, parent_text_transform, builder);
                builder.push_text(text);
            }
            NodeData::Comment { .. } => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            }
            NodeData::Document(_) => unreachable!(),
        }
    }
}

/// BGMail: line height quirk state while building an inline context.
struct LineHeightQuirk {
    enabled: bool,
    /// Whether nothing (no text, no box) precedes on the current line since
    /// the start or the last `<br>`.
    line_empty: bool,
}

/// BGMail: the (limited-)quirks line height calculation quirk
/// (quirks.spec.whatwg.org §3.3): an inline box with no text of its own and
/// no vertical padding or border sizes lines as if its `line-height` were 0,
/// so `<p><font size=1>text</font></p>` lines are as tall as the small text,
/// not the paragraph's strut. Like Blink, only the box directly holding the
/// text counts, not its ancestors.
fn line_height_quirk_applies(
    nodes: &crate::NodeTree,
    node: &crate::Node,
    style: &style::properties::ComputedValues,
) -> bool {
    let border = style.get_border();
    let padding = style.get_padding();
    // The computed width of a `none` border is still its declared width.
    let no_border = |style: BorderStyle, width: app_units::Au| {
        style.none_or_hidden() || width == app_units::Au(0)
    };
    no_border(border.border_top_style, border.border_top_width.0)
        && no_border(border.border_bottom_style, border.border_bottom_width.0)
        && padding
            .padding_top
            .0
            .to_length()
            .is_some_and(|l| l.px() == 0.0)
        && padding
            .padding_bottom
            .0
            .to_length()
            .is_some_and(|l| l.px() == 0.0)
        && !has_direct_text(nodes, node)
}

/// Whether `node` has a text child that is not just collapsible whitespace.
fn has_direct_text(nodes: &crate::NodeTree, node: &crate::Node) -> bool {
    node.children.iter().any(|&id| match &nodes[id].data {
        NodeData::Text(data) => !data.content.chars().all(|c| c.is_ascii_whitespace()),
        _ => false,
    })
}

/// BGMail: marks inline boxes that are spacers for an inline element's
/// horizontal margin + border + padding, not DOM nodes.
pub(crate) const SPACER_BIT: u64 = 1 << 63;

/// Whether an inline box id is a spacer (see [`SPACER_BIT`]).
pub(crate) fn is_spacer(id: u64) -> bool {
    id & SPACER_BIT != 0
}

fn spacer_box(node_id: u64, end: bool, width: f32) -> InlineBox {
    InlineBox {
        id: SPACER_BIT | (node_id << 1) | end as u64,
        kind: InlineBoxKind::InFlow,
        index: 0,
        width,
        height: 0.0,
        baseline: None,
        vertical_align: Default::default(),
    }
}

/// Horizontal (start, end) space of an inline element: margin + border +
/// padding, percentages resolved against zero.
fn inline_edge_space(style: &style::properties::ComputedValues) -> (f32, f32) {
    use style::values::computed::Length;
    let zero = Length::new(0.0);
    let padding = style.get_padding();
    let margin = style.get_margin();
    let border = style.get_border();
    let side = |w: style::values::computed::Au, s: style::values::computed::BorderStyle| {
        if s.none_or_hidden() {
            0.0
        } else {
            w.to_f32_px()
        }
    };
    let margin_px = |m: &style::values::computed::Margin| match m {
        style::values::generics::length::GenericMargin::LengthPercentage(lp) => {
            lp.resolve(zero).px()
        }
        _ => 0.0,
    };
    let start = margin_px(&margin.margin_left)
        + side(border.border_left_width.0, border.border_left_style)
        + padding.padding_left.0.resolve(zero).px();
    let end = margin_px(&margin.margin_right)
        + side(border.border_right_width.0, border.border_right_style)
        + padding.padding_right.0.resolve(zero).px();
    (start.max(0.0), end.max(0.0))
}
