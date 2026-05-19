a = true;
b = false;
c = true(2, 3);
d = false(1, 2);
e = islogical(a);
f = islogical(c);
g = string(true);
h = compose("mask=%s", false);
i_like = true(like=a);
j_like = false(2, 3, like=c);
k_like_size = size(j_like);
try
    i = logical(NaN);
catch err_nan
    i = err_nan.identifier;
    i_msg = err_nan.message;
end
try
    j = logical(1 + 2i);
catch err_complex
    j = err_complex.identifier;
    j_msg = err_complex.message;
end
try
    k = logical([1 NaN 0]);
catch err_nan_matrix
    k = err_nan_matrix.identifier;
    k_msg = err_nan_matrix.message;
end
try
    like_err = true(2, like=1);
catch err_like
    like_err = err_like.identifier;
    like_err_msg = err_like.message;
end
l_real = isreal(true);
m_real = isreal('ab');
n_real = isreal("ab");
o_real = isreal(complex(12));
p_real = isreal(complex([1 2]));
q_real = isreal(["ab", "cd"]);
