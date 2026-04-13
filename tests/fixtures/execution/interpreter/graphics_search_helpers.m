f = figure(37);
top = subplot(211);
line = plot([0, 1, 2], [0, 1, 4]);
hold on
dots = scatter([0, 1, 2], [1, 0, 1]);
set(line, 'DisplayName', "Curve");
set(dots, 'Visible', 'off');

bottom = subplot(212);
mesh_handle = mesh([0, 1; 1, 0]);
set(bottom, 'Box', 'off');

all_from_figure = findobj(f);
axes_only = findobj(f, 'Type', 'axes');
line_only = findobj(f, 'Type', 'line');
mesh_only = findall(f, 'Type', 'mesh');
from_axes = findobj(top);
current_all = findall();
implicit_curve = findobj('DisplayName', 'Curve');
hidden_objects = findall(f, 'Visible', 'off');
box_off_axes = findobj(f, 'Box', 'off');

delete(dots);
after_delete = findobj(f);
top_after_delete = findobj(top);
