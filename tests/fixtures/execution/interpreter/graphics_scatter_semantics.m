f = figure(64);
x = [0, 1, 2];
y = [0, 1, 0];
z = [1, 2, 3];

h = scatter(x, y, [36, 64, 100], [1, 0, 0; 0, 1, 0; 0, 0, 1], 'filled');
h_type = get(h, 'Type');
h_marker = get(h, 'Marker');
h_edge = get(h, 'MarkerEdgeColor');
h_face = get(h, 'MarkerFaceColor');
h_cdata = get(h, 'CData');
h_size = get(h, 'SizeData');
set(h, 'CData', [10; 20; 30], 'SizeData', [49, 81, 121]);
h_cdata_updated = get(h, 'CData');
h_size_updated = get(h, 'SizeData');
h_x = get(h, 'XData');
h_y = get(h, 'YData');

h3 = scatter3(x, y, z, 49, [0.5, 0, 0.5], 'filled');
h3_type = get(h3, 'Type');
h3_marker = get(h3, 'Marker');
h3_edge = get(h3, 'MarkerEdgeColor');
h3_face = get(h3, 'MarkerFaceColor');
h3_cdata = get(h3, 'CData');
h3_size = get(h3, 'SizeData');
h3_z = get(h3, 'ZData');
