f = figure(49);
curve = plot([0, 1, 2], [0, 1, 0], 'r--o');
curve_color = get(curve, 'Color');
curve_style = get(curve, 'LineStyle');
curve_marker = get(curve, 'Marker');

hold on
arc = plot3([0, 1, 2], [0, 1, 0], [1, 2, 3], 'b:s');
arc_color = get(arc, 'Color');
arc_style = get(arc, 'LineStyle');
arc_marker = get(arc, 'Marker');
