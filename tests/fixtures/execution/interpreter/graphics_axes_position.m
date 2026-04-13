f = figure(44);
top = subplot(211);
top_position = get(top, 'Position');

free = axes('Position', [0.55, 0.15, 0.25, 0.3]);
free_position = get(free, 'Position');
plot([0, 1], [1, 0]);
current_free = gca();

set(free, 'Position', [0.5, 0.2, 0.3, 0.35]);
updated_free_position = get(free, 'Position');

selected = axes(top);
current_top = gca();
children = allchild(f);
