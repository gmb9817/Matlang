f = figure(38);
top = subplot(211);
line = plot([0, 1, 2], [0, 1, 4]);
hold on
dots = scatter([0, 1, 2], [1, 0, 1]);

lower = subplot(212);
mesh_handle = mesh([0, 1; 1, 0]);

mixed_types = get([line, dots; top, f], 'Type');
set([line, dots], 'DisplayName', "Series");
display_names = get([line, dots], 'DisplayName');
set([line, dots], 'Visible', 'off');
visibility_off = get([line, dots], 'Visible');
set([line, dots], 'Visible', 'on');
visibility_on = get([line, dots], 'Visible');
set([top, lower], 'Box', 'off');
boxes = get([top, lower], 'Box');
