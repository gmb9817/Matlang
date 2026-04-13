f1 = figure(60, 'Name', 'Hidden Figure', 'Visible', 'off');
plot([0, 1], [0, 1]);
f2 = figure(61, 'Name', 'Docked Figure', 'WindowStyle', 'docked');
plot([0, 1], [1, 0]);

f1_visible_before_close = get(f1, 'Visible');
f2_style_before_close = get(f2, 'WindowStyle');

status_one = close(f1);
f1_alive = isgraphics(f1, 'figure');
current_after_one = gcf();

status_all = close('all');
f2_alive = isgraphics(f2, 'figure');
current_is_new = isgraphics(gcf(), 'figure');
