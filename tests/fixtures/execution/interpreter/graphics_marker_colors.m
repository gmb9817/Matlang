f = figure(53);
h = plot([0, 1, 2], [0, 1, 0], 'o');
default_edge = get(h, 'MarkerEdgeColor');
default_face = get(h, 'MarkerFaceColor');

set(h, 'MarkerFaceColor', [1, 0, 0], 'MarkerEdgeColor', [0, 0, 0]);
filled_edge = get(h, 'MarkerEdgeColor');
filled_face = get(h, 'MarkerFaceColor');

line_h = line('XData', [0, 1], 'YData', [1, 0], 'Marker', 'square', 'MarkerFaceColor', 'g', 'MarkerEdgeColor', 'b');
line_edge = get(line_h, 'MarkerEdgeColor');
line_face = get(line_h, 'MarkerFaceColor');
