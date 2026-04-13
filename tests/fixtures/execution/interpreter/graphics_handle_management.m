f = figure(35);
top = subplot(211);
line = plot([0, 1, 2], [0, 1, 4]);
hold on
dots = scatter([0, 1, 2], [1, 0, 1]);
top_hold_after_command = ishold(top);

bottom = subplot(212);
mesh_handle = mesh([0, 1; 1, 0]);
bottom_hold_initial = ishold(bottom);
hold(top, 'off');
top_hold_after_target_off = ishold(top);
bottom_hold_after_target_off = ishold(bottom);
hold
bottom_hold_after_toggle = ishold(bottom);
hold off
bottom_hold_after_off = ishold(bottom);
hold(top, 'all');
top_hold_after_all = ishold(top);

is_line = ishghandle(line);
is_matrix = ishghandle([line, dots; top, f]);
is_line_graphics = isgraphics(line);
is_top_axes = isgraphics(top, 'axes');
is_line_line = isgraphics(line, 'line');
is_line_axes = isgraphics(line, 'axes');
invalid_handle = isgraphics(99999);

delete(dots);
top_children_after_delete = allchild(top);
dots_after_delete = isgraphics(dots);

delete([line, mesh_handle]);
top_children_empty = allchild(top);
bottom_children_empty = allchild(bottom);

delete(f);
figure_after_delete = isgraphics(f);
current_new = gcf();
current_is_figure = isgraphics(current_new, 'figure');
