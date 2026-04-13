function y = outer()
x = 1;
function inner()
x = 2;
end
inner();
y = x;
end
