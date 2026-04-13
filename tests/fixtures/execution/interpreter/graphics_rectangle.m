f = figure(42);
rect = rectangle('Position', [0.5, 0.25, 1.5, 1], 'EdgeColor', 'r', 'FaceColor', 'g', 'LineWidth', 3, 'LineStyle', '--');

rect_type = get(rect, 'Type');
rect_position = get(rect, 'Position');
rect_edge = get(rect, 'EdgeColor');
rect_face = get(rect, 'FaceColor');
rect_parent = ancestor(rect, 'axes');

set(rect, 'Position', [1, 0.5, 1, 0.75]);
set(rect, 'FaceColor', 'none');
updated_position = get(rect, 'Position');
updated_face = get(rect, 'FaceColor');

found_rect = findobj(f, 'Type', 'rectangle');
red_edge_rects = findall(f, 'EdgeColor', [1, 0, 0]);
