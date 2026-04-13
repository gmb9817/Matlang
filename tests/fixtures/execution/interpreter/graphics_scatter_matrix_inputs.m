x = [0; 1; 2];
y = [0, 1; 1, 0; 0, 1];
s = [36, 49; 64, 81; 100, 121];
c = [1, 10; 2, 20; 3, 30];

f = figure(65);
h = scatter(x, y, s, c, 'filled');
h_types = get(h, 'Type');
h1_size = get(h(1), 'SizeData');
h2_size = get(h(2), 'SizeData');
h1_cdata = get(h(1), 'CData');
h2_cdata = get(h(2), 'CData');
h1_y = get(h(1), 'YData');
h2_y = get(h(2), 'YData');

z = [1, 4; 2, 5; 3, 6];
h3 = scatter3(x, y, z, 25, [0.2, 0.4, 0.8], 'filled');
h3_type = get(h3, 'Type');
h3_1_z = get(h3(1), 'ZData');
h3_2_z = get(h3(2), 'ZData');
h3_1_cdata = get(h3(1), 'CData');
