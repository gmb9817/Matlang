f = figure(50);
marker_only = plot([0, 1, 2], [0, 1, 0], 'k^');
marker_only_color = get(marker_only, 'Color');
marker_only_style = get(marker_only, 'LineStyle');
marker_only_marker = get(marker_only, 'Marker');

hold on
dashdot = plot([0, 1, 2], [1, 0, 1], 'm-.x');
dashdot_color = get(dashdot, 'Color');
dashdot_style = get(dashdot, 'LineStyle');
dashdot_marker = get(dashdot, 'Marker');

property_marker = plot([0, 1, 2], [2, 1, 2], 'Marker', 'hexagram', 'LineStyle', '-.');
property_marker_style = get(property_marker, 'LineStyle');
property_marker_marker = get(property_marker, 'Marker');

arc = plot3([0, 1, 2], [0, 1, 0], [1, 2, 3], 'gd-.');
arc_color = get(arc, 'Color');
arc_style = get(arc, 'LineStyle');
arc_marker = get(arc, 'Marker');
