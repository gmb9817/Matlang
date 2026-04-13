f = figure(39);
curve = plot([0, 1, 2], [0, 1, 0]);
label = text(1, 0.5, "Peak");

label_type = get(label, 'Type');
label_string = get(label, 'String');
label_position = get(label, 'Position');
set(label, 'String', 'Center');
set(label, 'Position', [1.5, 0.25]);
updated_string = get(label, 'String');
updated_position = get(label, 'Position');
text_found = findobj(f, 'Type', 'text');
string_found = findall('String', 'Center');
text_ancestor = ancestor(label, 'text');
axes_parent = ancestor(label, 'axes');
delete(label);
after_delete = isgraphics(label);
