sin_handle = str2func("sin");
sin_name = func2str(sin_handle);
sin_value = sin_handle(pi / 2);

plus_handle = str2func('plus');
plus_name = func2str(plus_handle);
plus_value = plus_handle(2, 3);

cos_handle = str2func("@cos");
cos_name = func2str(cos_handle);
cos_value = cos_handle(0);

helper_handle = str2func("helper");
helper_name = func2str(helper_handle);
helper_value = helper_handle(5);

direct_builtin_name = func2str(@sin);
direct_helper_name = func2str(@helper);

function y = helper(x)
y = x + 1;
end
