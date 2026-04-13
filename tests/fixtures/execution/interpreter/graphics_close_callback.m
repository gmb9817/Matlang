f = figure(80);
set(f, 'CloseRequestFcn', @open_callback_figure, 'ResizeFcn', @mark_resize);

close_request = get(f, 'CloseRequestFcn');
resize_request = get(f, 'ResizeFcn');
fig_props = get(f);
close_request_struct = fig_props.CloseRequestFcn;
resize_request_struct = fig_props.ResizeFcn;

status_handle = close(f);
alive_after_handle = isgraphics(f, 'figure');
current_after_handle = gcf();
callback_figure_name = get(82, 'Name');
callback_figure_alive = isgraphics(82, 'figure');
closereq();
callback_figure_alive_after_closereq = isgraphics(82, 'figure');

set(f, 'CloseRequestFcn', []);
status_raw = close(f);
alive_after_raw = isgraphics(f, 'figure');

g = figure(81);
set(g, 'CloseRequestFcn', "closereq");
status_text = close(g);
alive_after_text = isgraphics(g, 'figure');

function open_callback_figure()
figure(82, 'Name', 'Callback Ran');
end

function mark_resize()
end
