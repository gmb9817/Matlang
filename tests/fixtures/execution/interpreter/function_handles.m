x = 5;
a = (@(z) z + x)(2);
b = (@helper)(3);
function y = helper(v)
y = v + 4;
end
