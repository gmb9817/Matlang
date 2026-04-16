s = cat(3, [struct("value", 1), struct("value", 2)], [struct("value", 3), struct("value", 4)]);
a = reshape([s.value], size(s));
b = size(a);
s.value = cat(3, [10, 20], [30, 40]);
c = reshape([s.value], size(s));
d = size(c);

expr_failed = false;
try
    expr_bad = s(1:2).value(1);
catch err
    expr_failed = true;
end

assign_msg = "none";
try
    s(1:2).value(1) = [50 60];
catch err
    assign_msg = err.message;
end
