a = logical(0);
b = logical(2);
c = logical([0 1; -3 0]);
try
    d = logical(1i);
catch err1
    d = err1.identifier;
    d_msg = err1.message;
end
try
    e = logical([1i 0; 2-3i 0]);
catch err2
    e = err2.identifier;
    e_msg = err2.message;
end
f = logical('ab');
g = logical([]);
g_class = class(g);
g_f = flip(g);
g_f_class = class(g_f);
g_t = g';
g_t_class = class(g_t);
h_empty = logical(zeros(0, 2));
h_empty_class = class(h_empty);
h_scalar_rep0 = repelem(true, 0);
h_scalar_rep0_class = class(h_scalar_rep0);
h_empty_rot = rot90(h_empty);
h_empty_rot_class = class(h_empty_rot);
h_empty_r = reshape(h_empty, [1 0]);
h_empty_r_class = class(h_empty_r);
h_empty_rep0 = repelem(h_empty, 0);
h_empty_rep0_class = class(h_empty_rep0);
h_empty_shift = circshift(h_empty, 0);
h_empty_shift_class = class(h_empty_shift);
h = logical(true);
i = logical(false);
