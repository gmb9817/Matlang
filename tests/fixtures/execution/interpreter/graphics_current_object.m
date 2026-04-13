none = gco();
f = figure(50);
ax = subplot(211);
after_axes = gco();
fig_current_axes = get(f, 'CurrentAxes');
fig_current_object_axes = get(f, 'CurrentObject');

curve = plot([0, 1, 2], [0, 1, 0]);
after_plot = gco();
fig_current_object_line = get(f, 'CurrentObject');

txt = text(1, 0.5, "Peak");
after_text = gco();
fig_current_object_text = get(f, 'CurrentObject');

lower = subplot(212);
after_lower = gco();
copy = copyobj(curve, lower);
after_copy = gco();
fig_current_object_copy = get(f, 'CurrentObject');

delete(copy);
after_delete = gco();
fig_current_object_after_delete = get(f, 'CurrentObject');
