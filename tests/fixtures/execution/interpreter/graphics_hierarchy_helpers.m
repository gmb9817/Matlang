f = figure(33);
top = subplot(211);
set(top, 'Title', "Keep");
line = plot([0, 1, 2], [0, 1, 4]);
hold on
dots = scatter([0, 1, 2], [1, 0, 1]);

bottom = subplot(212);
mesh_handle = mesh([0, 1; 1, 0]);

figure_children_before = allchild(f);
top_children_before = allchild(top);
bottom_children_before = allchild(bottom);
line_axes = ancestor(line, 'axes');
line_figure = ancestor(line, 'figure');
top_figure = ancestor(top, 'figure');
missing_axes = ancestor(f, 'axes');

cla(top);
top_children_after = allchild(top);
top_title_after = get(top, 'Title');

subplot(212);
cla();
bottom_children_after = allchild(bottom);
figure_children_after = allchild(f);
current_after = gca();

set(top, 'Title', "Reset Me", 'XLim', [-1, 3], 'YLim', [0, 2], 'View', [10, 20], 'Box', 'off', 'Grid', 'on', 'Hold', 'on', 'Position', [0.1, 0.2, 0.3, 0.4]);
cla(top, 'reset');
top_title_reset = get(top, 'Title');
top_xlim_reset = get(top, 'XLim');
top_ylim_reset = get(top, 'YLim');
top_view_reset = get(top, 'View');
top_box_reset = get(top, 'Box');
top_grid_reset = get(top, 'Grid');
top_hold_reset = get(top, 'Hold');
top_position_reset = get(top, 'Position');
