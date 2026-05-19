a = pi;
b = eps;
c = Inf;
d = NaN;
e = realmin;
f = realmax;
g = flintmax;
h = inf(2);
i = NaN(1, 3);
j = pi + 1;
k = eps * 2;
l = realmax();
m = realmin();
n = flintmax();
o = inf;
p = nan;
q = inf(1, 2, like=complex(0, 1));
q_isreal = isreal(q);
r = nan([1, 3], like=complex(0, 1));
r_isreal = isreal(r);
try
    s = inf(2, like=true);
catch err_like1
    s = err_like1.identifier;
    s_msg = err_like1.message;
end
try
    t = nan(2, like=true);
catch err_like2
    t = err_like2.identifier;
    t_msg = err_like2.message;
end
