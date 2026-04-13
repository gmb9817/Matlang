x = 2;
switch x
case 1
y = zeros(1, 1);
case x
y = userfunc(x);
otherwise
y = x;
end
