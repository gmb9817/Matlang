f = figure(32);
top = subplot(211);
line = plot([0, 1, 2], [0, 1, 4]);
hold on
dots = scatter([0, 1, 2], [1, 0, 1]);

bottom = subplot(212);
mesh_handle = mesh([0, 1; 1, 0]);

figure_type = get(f, 'Type');
figure_children = get(f, 'Children');
top_type = get(top, 'Type');
top_parent = get(top, 'Parent');
top_children = get(top, 'Children');
bottom_children = get(bottom, 'Children');
line_type = get(line, 'Type');
line_parent = get(line, 'Parent');
dots_parent = get(dots, 'Parent');
mesh_parent = get(mesh_handle, 'Parent');
