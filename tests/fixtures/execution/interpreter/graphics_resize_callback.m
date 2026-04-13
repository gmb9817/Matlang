f = figure(90, 'Name', 'Initial Figure');
set(f, 'ResizeFcn', @mark_resize);

resize_request_initial = get(f, 'ResizeFcn');
name_before_position = get(f, 'Name');

set(f, 'Position', [5, 6, 320, 240]);
name_after_position = get(f, 'Name');
position_after_position = get(f, 'Position');

fig_props = get(f);
resize_request_struct = fig_props.ResizeFcn;

set(f, 'Name', 'Reset Figure');
set(f, 'ResizeFcn', []);
resize_request_cleared = get(f, 'ResizeFcn');

set(f, 'Position', [7, 8, 330, 250]);
name_after_cleared = get(f, 'Name');
position_after_cleared = get(f, 'Position');

function mark_resize()
set(gcf(), 'Name', 'Resize Callback');
end
