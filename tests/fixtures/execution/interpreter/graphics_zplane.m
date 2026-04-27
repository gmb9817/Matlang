f = figure(63);
[hz, hp, ht] = zplane([0; -1], [0.5 + 0.5i; 0.5 - 0.5i]);

hz1 = hz(1);
hp1 = hp(1);
ax = ht(1);
unit_circle = ht(2);

zero_marker = get(hz1, 'Marker');
pole_marker = get(hp1, 'Marker');
unit_style = get(unit_circle, 'LineStyle');
unit_marker = get(unit_circle, 'Marker');
x_label = get(ax, 'XLabel');
y_label = get(ax, 'YLabel');

zero_handles = findobj(f, 'Marker', 'o');
pole_handles = findobj(f, 'Marker', 'x');

f2 = figure(64);
[hz_rep, hp_rep, ht_rep] = zplane([0.2; 0.2; 0.2], [0.5; 0.5]);
rep_zero_text = get(ht_rep(3), 'String');
rep_pole_text = get(ht_rep(4), 'String');
rep_zero_type = get(ht_rep(3), 'Type');
rep_pole_type = get(ht_rep(4), 'Type');
rep_text_handles = findobj(f2, 'Type', 'text');

f3 = figure(65);
[hz_mat, hp_mat, ht_mat] = zplane([0.1 + 0.1i, 0.2 + 0.2i; 0.1 - 0.1i, 0.2 - 0.2i], [0.5 + 0.1i, 0.6 + 0.1i; 0.5 - 0.1i, 0.6 - 0.1i]);
hz_mat1_color = get(hz_mat(1), 'Color');
hp_mat1_color = get(hp_mat(1), 'Color');
hz_mat2_color = get(hz_mat(2), 'Color');
hp_mat2_color = get(hp_mat(2), 'Color');
