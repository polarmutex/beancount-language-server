pub fn text_for_tree_sitter_node(
    source: &ropey::Rope,
    node: &tree_sitter::Node,
) -> std::string::String {
    let start = source.byte_to_char(node.start_byte() as usize);
    let end = source.byte_to_char(node.end_byte() as usize);
    let slice = source.slice(start..end);
    slice.into()
}
