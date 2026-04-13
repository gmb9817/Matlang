f = figure(45);
ax = subplot(211);
line_handle = line([0, 1, 2], [0, 1, 0]);
txt = text(1, 0.5, "Hello");

fig_props = get(f);
fig_type = fig_props.Type;
fig_children = fig_props.Children;

ax_props = get(ax);
ax_position = ax_props.Position;
ax_type = ax_props.Type;

line_props = get(line_handle);
line_type = line_props.Type;
line_color = line_props.Color;

obj_props = get([line_handle, txt]);
line_struct_type = obj_props{1}.Type;
text_struct_string = obj_props{2}.String;
