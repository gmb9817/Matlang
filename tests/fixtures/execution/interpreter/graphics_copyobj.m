f = figure(46);
top = subplot(211);
base = plot([0, 1, 2], [0, 1, 0]);
set(base, 'DisplayName', "Base");

bottom = subplot(212);
copy_one = copyobj(base, bottom);
copy_one_parent = ancestor(copy_one, 'axes');
copy_one_name = get(copy_one, 'DisplayName');
bottom_children = allchild(bottom);

copy_axes = copyobj(top, f);
copy_axes_parent = ancestor(copy_axes, 'figure');
copy_axes_position = get(copy_axes, 'Position');
copy_axes_children = allchild(copy_axes);
copy_axes_child_type = get(copy_axes_children, 'Type');

copy_many = copyobj([base, copy_one], copy_axes);
copy_many_types = get(copy_many, 'Type');
