a = flipud([1 2; 3 4]);
b = fliplr([1 2; 3 4]);
c = rot90([1 2; 3 4]);
d = rot90([1 2; 3 4], 2);
e = rot90([1 2; 3 4], -1);
f = circshift([1 2; 3 4], 1);
g = circshift([1 2; 3 4], [1, -1]);
h = circshift([1, 2, 3], 1);
i = fliplr('abc');
j = flipud('abc');
k = rot90({1, 2; 3, 4});
l = circshift({1, 2; 3, 4}, [0, 1]);
m = circshift(cat(3, [1 2; 3 4], [5 6; 7 8]), 1, 3);
n = circshift(cat(3, [1 2; 3 4], [5 6; 7 8]), [1 0 1]);
o = circshift(cat(3, {1 2; 3 4}, {5 6; 7 8}), 1, 3);
p = circshift('abc', 1, 3);
q = rot90(cat(3, [1 2; 3 4], [5 6; 7 8]));
r = rot90(cat(3, {1 2; 3 4}, {5 6; 7 8}), -1);
s = pagetranspose(cat(3, [1 2; 3 4], [5 6; 7 8]));
t = pagectranspose(cat(3, [1+1i 2+2i; 3+3i 4+4i], [5+5i 6+6i; 7+7i 8+8i]));
u = pagetranspose(cat(3, {1 2; 3 4}, {5 6; 7 8}));
v = ctranspose({1, 2; 3, 4});
wtmp = {1, 2; 3, 4};
w = wtmp';
empty_perm = permute(zeros(0, 2), [2 1]);
empty_perm_size = size(empty_perm);
empty_cell_perm = permute(num2cell(zeros(0, 2)), [2 1]);
empty_cell_perm_size = size(empty_cell_perm);
try
    frac_shift = circshift([1 2; 3 4], 1.5);
catch err
    frac_shift_msg = char(err.message);
    clear err
end
try
    inf_shift = circshift([1 2; 3 4], [1 Inf]);
catch err
    inf_shift_msg = char(err.message);
    clear err
end
nd = cat(3, [1 2; 3 4], [5 6; 7 8]);
try
    bad_ctranspose = nd';
catch err
    bad_ctranspose_msg = char(err.message);
    clear err
end
try
    bad_transpose = nd.';
catch err
    bad_transpose_msg = char(err.message);
    clear err
end
